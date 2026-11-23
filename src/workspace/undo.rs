pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;

use crate::workspace::undo::engine::{append_operation, apply_restore, capture_state, head_label, plan_restore, read_oplog, ref_label, undo_stacks};
use crate::workspace::undo::types::{Change, OpKind, Operation, RepoState};

/// `rgit undo`: puts the repository back the way it was before the last recorded operation
/// that hasn't been undone yet, after showing what that operation was and what will change.
/// Refuses when something the operation touched has changed again since (unless `force`).
pub fn undo(dry_run: bool, force: bool) -> Result<()> {
    let ops = read_oplog();
    let (done, _) = undo_stacks(&ops);
    let Some(op) = done.last().and_then(|id| ops.iter().find(|o| o.id == *id)) else {
        println!("Nothing to undo: rgit hasn't recorded any operation here yet.");
        println!("(Recording starts with the first state-changing rgit command; `rgit undo --list` shows the log.)");
        return Ok(());
    };
    step(op, Direction::Undo, dry_run, force)
}

/// `rgit redo`: re-applies the operation most recently undone, if no new command ran since.
pub fn redo(dry_run: bool, force: bool) -> Result<()> {
    let ops = read_oplog();
    let (_, undone) = undo_stacks(&ops);
    let Some(op) = undone.last().and_then(|id| ops.iter().find(|o| o.id == *id)) else {
        println!("Nothing to redo: no operation has been undone since the last command.");
        return Ok(());
    };
    step(op, Direction::Redo, dry_run, force)
}

/// `rgit undo --list`: the operations `undo` would take back (newest first) and those `redo`
/// would bring back.
pub fn list() -> Result<()> {
    let ops = read_oplog();
    let (done, undone) = undo_stacks(&ops);
    if done.is_empty() && undone.is_empty() {
        println!("The operation log is empty: nothing to undo yet.");
        return Ok(());
    }
    let find = |id: &u64| ops.iter().find(|o| o.id == *id);
    if !done.is_empty() {
        println!("Undoable operations (newest first; `rgit undo` takes back the top one):");
        for (i, op) in done.iter().rev().filter_map(find).enumerate() {
            if i == 20 {
                println!("  ... and {} older", done.len() - 20);
                break;
            }
            println!("  #{:<4} {:>14}  {}{}", op.id, ago(op.time), op.command, summary(&op.before, &op.after));
        }
    }
    if !undone.is_empty() {
        println!("Undone (`rgit redo` brings back the top one; a new command forgets these):");
        for op in undone.iter().rev().filter_map(find) {
            println!("  #{:<4} {:>14}  {}{}", op.id, ago(op.time), op.command, summary(&op.before, &op.after));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    Undo,
    Redo,
}

/// Previews and (unless `dry_run`) performs one undo or redo of `op`, then logs it.
fn step(op: &Operation, dir: Direction, dry_run: bool, force: bool) -> Result<()> {
    let (from, to) = match dir {
        Direction::Undo => (&op.after, &op.before),
        Direction::Redo => (&op.before, &op.after),
    };
    let current = capture_state(false)?.state;
    let plan = plan_restore(from, to, &current)?;
    let verb = if dir == Direction::Undo { "Undoing" } else { "Redoing" };
    let verb = if dry_run { if dir == Direction::Undo { "Would undo" } else { "Would redo" } } else { verb };

    println!("{} #{}: rgit {}  ({})", verb, op.id, op.command, ago(op.time));
    if plan.changes.is_empty() {
        println!("  (the repository already looks the way it would afterwards)");
    }
    for change in &plan.changes {
        for line in describe_change(change) {
            println!("  - {}", line);
        }
    }

    if !plan.conflicts.is_empty() && !force {
        eprintln!();
        eprintln!("error: can't {} #{} cleanly: some of what it changed has changed again since:", if dir == Direction::Undo { "undo" } else { "redo" }, op.id);
        for c in &plan.conflicts {
            eprintln!("  - {}", c);
        }
        eprintln!("fix:   `rgit stash` (or commit) your new changes first and try again,");
        eprintln!("       or `rgit {} --force` to overwrite them.", if dir == Direction::Undo { "undo" } else { "redo" });
        crate::ui::exit(1);
    }
    if dry_run {
        if !plan.conflicts.is_empty() {
            println!("Note: {} of these changed again since; a real run needs --force.", plural(plan.conflicts.len(), "thing", "things"));
        }
        return Ok(());
    }
    if !plan.conflicts.is_empty() {
        println!("  (--force: overwriting {} that changed since)", plural(plan.conflicts.len(), "thing", "things"));
    }

    let touches_files = plan.changes.iter().any(|c| matches!(c, Change::Files { .. }));
    let before = capture_state(touches_files)?;
    let label = if dir == Direction::Undo { "undo" } else { "redo" };
    crate::core::reflog::set_reflog_action(&format!("{}: {}", label, op.command));
    apply_restore(&plan)?;
    let after = capture_state(touches_files)?;
    if before.state.index_key != after.state.index_key
        && let Some(bytes) = &before.index_bytes
    {
        crate::workspace::undo::engine::store_write(bytes)?;
    }
    let kind = if dir == Direction::Undo { OpKind::Undo { target: op.id } } else { OpKind::Redo { target: op.id } };
    append_operation(kind, &format!("{} #{}", label, op.id), before.state, after.state)?;

    match dir {
        Direction::Undo => println!("Done. `rgit redo` brings it back."),
        Direction::Redo => println!("Done. `rgit undo` takes it back again."),
    }
    Ok(())
}

/// One or more preview lines for a planned change.
fn describe_change(change: &Change) -> Vec<String> {
    let commit = |h: &str| {
        let subject = crate::core::commit::commit_subject_line(h).unwrap_or_default();
        if subject.is_empty() { h[..7].to_string() } else { format!("{} \"{}\"", &h[..7], truncate(&subject, 50)) }
    };
    match change {
        Change::Head { from, to } => vec![format!("HEAD: {} -> {}", head_label(from), head_label(to))],
        Change::Ref { name, from, to } => vec![match (from, to) {
            (None, Some(t)) => format!("{}: recreated at {}", ref_label(name), commit(t)),
            (Some(f), None) => format!("{}: removed (was at {})", ref_label(name), commit(f)),
            (Some(f), Some(t)) => format!("{}: {} -> {}", ref_label(name), &f[..7], commit(t)),
            (None, None) => String::new(),
        }],
        Change::Index { to } => vec![match to {
            Some(_) => "index (staging area): put back as it was".to_string(),
            None => "index (staging area): removed".to_string(),
        }],
        Change::Stash { from_count, to_count, .. } => {
            vec![format!("stash list: {} -> {}", plural(*from_count, "entry", "entries"), plural(*to_count, "entry", "entries"))]
        }
        Change::StateFiles { from_names, to_names, .. } => {
            let op_name = |names: &[String]| -> Option<&'static str> {
                let has = |n: &str| names.iter().any(|x| x == n || x.starts_with(&format!("{}/", n)));
                if has("rebase-merge") || has("rebase-apply") || has("REBASE_TODO") { Some("rebase") }
                else if has("MERGE_HEAD") { Some("merge") }
                else if has("CHERRY_PICK_HEAD") { Some("cherry-pick") }
                else if has("REVERT_HEAD") { Some("revert") }
                else if has("BISECT_START") { Some("bisect") }
                else if has("RGIT_STASH_CONFLICT") { Some("stash apply") }
                else { None }
            };
            vec![match (op_name(from_names), op_name(to_names)) {
                (Some(a), None) => format!("the in-progress {} is cancelled (its state files are removed)", a),
                (None, Some(b)) => format!("the in-progress {} comes back", b),
                (Some(a), Some(b)) if a != b => format!("in-progress {} -> in-progress {}", a, b),
                // Only bookkeeping such as ORIG_HEAD changed: nothing worth a line.
                _ => return Vec::new(),
            }]
        }
        Change::Files { write, remove } => {
            let mut out = Vec::new();
            if !write.is_empty() {
                let names: Vec<String> = write.iter().map(|w| w.0.clone()).collect();
                out.push(format!("files restored: {}", join_list(&names, 6)));
            }
            if !remove.is_empty() {
                out.push(format!("files removed (created by the operation): {}", join_list(remove, 6)));
            }
            out
        }
    }
}

/// A short "  (branch 'main' moved, 2 files)" hint for the list view.
fn summary(before: &RepoState, after: &RepoState) -> String {
    let mut parts = Vec::new();
    if before.head != after.head {
        parts.push(format!("HEAD -> {}", head_label(&after.head)));
    }
    for name in before.refs.keys().chain(after.refs.keys()).collect::<std::collections::BTreeSet<_>>() {
        match (before.refs.get(name), after.refs.get(name)) {
            (a, b) if a == b => {}
            (None, Some(_)) => parts.push(format!("+{}", ref_label(name))),
            (Some(_), None) => parts.push(format!("-{}", ref_label(name))),
            _ => parts.push(format!("{} moved", ref_label(name))),
        }
    }
    if before.index_key != after.index_key {
        parts.push("index".into());
    }
    if before.worktree != after.worktree {
        parts.push("files".into());
    }
    if before.stash != after.stash {
        parts.push("stash".into());
    }
    if parts.is_empty() { String::new() } else { format!("   ({})", truncate(&parts.join(", "), 70)) }
}

/// "just now", "5 minutes ago", "3 days ago".
fn ago(time: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(time);
    let s = (now - time).max(0);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} ago", plural((s / 60) as usize, "minute", "minutes")),
        3600..=86399 => format!("{} ago", plural((s / 3600) as usize, "hour", "hours")),
        _ => format!("{} ago", plural((s / 86400) as usize, "day", "days")),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

fn join_list(items: &[String], max: usize) -> String {
    let mut out = items.iter().take(max).cloned().collect::<Vec<_>>().join(", ");
    if items.len() > max {
        out.push_str(&format!(" and {} more", items.len() - max));
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}...", s.chars().take(max).collect::<String>()) }
}
