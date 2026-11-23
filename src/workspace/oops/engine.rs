//! The detectors behind `rgit oops` (ideas file B, "I just did something wrong").
//!
//! Each detector looks at one kind of likely mistake (an operation half-done, a risky last
//! command, a commit on the wrong branch, a deleted branch, commits no branch points to any
//! more, a detached HEAD) and, when it sees one, suggests a fix made of plain rgit commands.
//! Every fix is either undoable with `rgit undo` or only adds refs, so following a suggestion
//! can never lose work.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::core::reflog::NULL_HASH;
use crate::core::refs;
use crate::ui::narrate::{commits_between, is_ancestor};
use crate::workspace::oops::types::{Fix, OopsContext};
use crate::workspace::undo::engine::{capture_state, plan_restore, undo_stacks};
use crate::workspace::undo::types::OpKind;

/// How far back (seconds) a command still counts as "just did".
const RECENT: i64 = 2 * 3600;
/// How far back HEAD's reflog is searched for commits that were left behind.
const LOST_WINDOW: i64 = 14 * 86400;

/// Reads everything the detectors need, once.
// Used by `collect_fixes`.
pub fn gather_context() -> Result<OopsContext> {
    let (branch, head) = match refs::resolve_head()? {
        refs::HeadState::Branch(b) => {
            let tip = refs::read_ref(&format!("refs/heads/{}", b))?;
            (Some(b), tip)
        }
        refs::HeadState::Detached(h) => (None, Some(h)),
    };
    let upstream = branch.as_ref().and_then(|b| {
        let get = |k: &str| crate::core::config::get_config_value(&format!("branch.{}.{}", b, k)).ok().flatten();
        let (remote, merge) = (get("remote")?, get("merge")?);
        let short = merge.strip_prefix("refs/heads/").unwrap_or(&merge).to_string();
        let name = if remote == "." { short.clone() } else { format!("{}/{}", remote, short) };
        let refname = if remote == "." { format!("refs/heads/{}", short) } else { format!("refs/remotes/{}/{}", remote, short) };
        refs::read_ref(&refname).ok().flatten().map(|tip| (name, tip))
    });
    let changes = crate::core::commit::tracked_worktree_changes().unwrap_or_default();
    let snapshot = crate::ui::narrate::take_snapshot(false);
    Ok(OopsContext {
        branch,
        head,
        upstream,
        ref_tips: refs::list_all_refs(),
        head_reflog: crate::core::reflog::read_reflog("HEAD").unwrap_or_default(),
        ops: crate::workspace::undo::engine::read_oplog(),
        dirty: !changes.is_empty() || !snapshot.staged.is_empty() || !snapshot.unmerged.is_empty(),
        in_progress: crate::ui::narrate::operation_in_progress(),
        now: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0),
    })
}

/// Runs every detector and returns their suggestions, most likely first. Suggestions whose
/// commands are identical are merged (the higher-ranked one is kept).
// Used by the `oops` command.
pub fn collect_fixes() -> Result<Vec<Fix>> {
    let ctx = gather_context()?;
    let mut fixes = Vec::new();
    fixes.extend(suggest_abort(&ctx));
    fixes.extend(suggest_redo(&ctx));
    fixes.extend(suggest_undo_last(&ctx));
    fixes.extend(suggest_uncommit(&ctx));
    fixes.extend(suggest_detached(&ctx));
    fixes.extend(suggest_move_commits(&ctx));
    let restores = suggest_restore_branches(&ctx);
    // Commits a deleted branch held are better brought back under that branch's name.
    let restored: BTreeSet<String> = restores.iter().filter_map(|f| f.commands.first().and_then(|c| c.last()).cloned()).collect();
    fixes.extend(restores);
    fixes.extend(suggest_lost_commits(&ctx).into_iter().filter(|f| !f.commands.first().and_then(|c| c.last()).is_some_and(|h| restored.contains(h))));
    fixes.sort_by_key(|f| std::cmp::Reverse(f.score));
    let mut seen = BTreeSet::new();
    fixes.retain(|f| seen.insert(f.commands.clone()));
    Ok(fixes)
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn subject(hash: &str) -> String {
    let s = crate::core::commit::commit_subject_line(hash).unwrap_or_default();
    if s.chars().count() > 50 { format!("{}...", s.chars().take(50).collect::<String>()) } else { s }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// The default branches commits rarely belong on directly.
fn is_default_branch(name: &str) -> bool {
    let configured = crate::core::config::get_config_value("init.defaultBranch").ok().flatten();
    name == "main" || name == "master" || configured.as_deref() == Some(name)
}

/// A branch name made from a commit subject: `fix-the-parser-crash` (at most 30 characters).
fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 30 {
            break;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A branch name that doesn't exist yet: `base`, else `base-2`, `base-3`, ...
fn free_branch_name(ctx: &OopsContext, base: &str) -> String {
    let base = if base.is_empty() || !refs::is_valid_branch_name(base) { "rescued" } else { base };
    let taken = |n: &str| ctx.ref_tips.contains_key(&format!("refs/heads/{}", n));
    if !taken(base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{}-{}", base, i)).find(|n| !taken(n)).unwrap_or_default()
}

/// The operation `rgit undo` would take back right now, if the log's newest entry is a command
/// and undoing it applies cleanly; detectors whose fix `undo` already covers stay quiet then.
fn clean_undo_target(ctx: &OopsContext) -> Option<&crate::workspace::undo::types::Operation> {
    if !matches!(ctx.ops.last().map(|o| &o.kind), Some(OpKind::Command)) {
        return None;
    }
    let (done, _) = undo_stacks(&ctx.ops);
    let op = done.last().and_then(|id| ctx.ops.iter().find(|o| o.id == *id))?;
    let current = capture_state(false).ok()?;
    let plan = plan_restore(&op.after, &op.before, &current.state).ok()?;
    (plan.conflicts.is_empty() && !plan.changes.is_empty()).then_some(op)
}

/// Whether `commit` can be reached from any branch, tag or remote-tracking ref.
fn reachable_from_any_ref(ctx: &OopsContext, commit: &str) -> bool {
    ctx.ref_tips.values().any(|tip| is_ancestor(commit, tip))
}

/// A merge, rebase, cherry-pick, revert or bisect is in progress: offer to cancel it.
// Used by `collect_fixes`.
fn suggest_abort(ctx: &OopsContext) -> Vec<Fix> {
    let Some(op) = ctx.in_progress.as_deref() else { return Vec::new() };
    let (command, what) = match op {
        "merge" => (args(&["merge", "--abort"]), "puts every file back the way it was before the merge"),
        "rebase" => (args(&["rebase", "--abort"]), "returns the branch to where it was before the rebase started"),
        "cherry-pick" => (args(&["cherry-pick", "--abort"]), "goes back to where you were before the cherry-pick"),
        "revert" => (args(&["revert", "--abort"]), "goes back to where you were before the revert"),
        "bisect" => (args(&["bisect", "reset"]), "ends the bisect and returns to the branch you started from"),
        _ => return Vec::new(),
    };
    vec![Fix {
        title: format!("Cancel the {} that's in progress", op),
        why: format!("A {} stopped halfway (usually on conflicts); cancelling it {}. To finish it instead: `rgit conflicts`.", op, what),
        commands: vec![command],
        score: 95,
    }]
}

/// The last thing logged was an undo: maybe it was the mistake.
// Used by `collect_fixes`.
fn suggest_redo(ctx: &OopsContext) -> Vec<Fix> {
    let Some(last) = ctx.ops.last() else { return Vec::new() };
    let OpKind::Undo { target } = last.kind else { return Vec::new() };
    let (_, undone) = undo_stacks(&ctx.ops);
    if undone.last() != Some(&target) {
        return Vec::new();
    }
    let Some(op) = ctx.ops.iter().find(|o| o.id == target) else { return Vec::new() };
    vec![Fix {
        title: format!("Take back your undo of `rgit {}`", op.command),
        why: format!("You ran `rgit undo` {}; `rgit redo` re-applies what it took back.", crate::workspace::undo::ago(last.time)),
        commands: vec![args(&["redo"])],
        score: if ctx.now - last.time < 900 { 80 } else { 55 },
    }]
}

/// Commands that throw work away or rewrite history: most likely to be "the mistake".
fn is_risky(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    let has = |w: &str| words.contains(&w);
    match words.first().copied().unwrap_or("") {
        "reset" => has("--hard") || has("--merge"),
        "branch" => has("-d") || has("-D") || has("--delete") || has("-m") || has("-M") || has("-f"),
        "tag" => has("-d") || has("--delete"),
        "stash" => words.get(1).is_some_and(|s| ["drop", "clear"].contains(s)),
        "commit" => has("--amend"),
        "checkout" => has("--") || has("-f") || has("--force"),
        "clean" | "restore" | "rm" | "rebase" | "merge" | "pull" | "cherry-pick" | "revert" | "banana-split" => true,
        _ => false,
    }
}

/// The most recent logged command (if `rgit undo` would still apply cleanly): offer to undo it.
// Used by `collect_fixes`.
fn suggest_undo_last(ctx: &OopsContext) -> Vec<Fix> {
    if !matches!(ctx.ops.last().map(|o| &o.kind), Some(OpKind::Command)) {
        return Vec::new();
    }
    let (done, _) = undo_stacks(&ctx.ops);
    let Some(op) = done.last().and_then(|id| ctx.ops.iter().find(|o| o.id == *id)) else { return Vec::new() };
    let Ok(current) = capture_state(false) else { return Vec::new() };
    let Ok(plan) = plan_restore(&op.after, &op.before, &current.state) else { return Vec::new() };
    // Undoing it would overwrite something changed since (`undo --force`): oops never suggests
    // a fix that can lose work.
    if plan.changes.is_empty() || !plan.conflicts.is_empty() {
        return Vec::new();
    }
    let lines: Vec<String> = plan.changes.iter().flat_map(crate::workspace::undo::describe_change).collect();
    let mut why = format!("Puts back what it changed: {}", lines.iter().take(3).cloned().collect::<Vec<_>>().join("; "));
    if lines.len() > 3 {
        why.push_str(&format!("; and {} more", lines.len() - 3));
    }
    why.push('.');
    let mut score: u32 = 50;
    if is_risky(&op.command) {
        score += 30;
    }
    if ctx.now - op.time < 900 {
        score += 10;
    }
    // Commits it made that are already pushed: undo only rewinds this clone.
    if let (Some(branch), Some((upstream, tip))) = (&ctx.branch, &ctx.upstream) {
        let refname = format!("refs/heads/{}", branch);
        if let Some(made) = op.after.refs.get(&refname)
            && op.before.refs.get(&refname) != Some(made)
            && is_ancestor(made, tip)
        {
            why.push_str(&format!(" Already pushed to {}: this only moves your local branch, the remote keeps it (`rgit revert` is the way for shared history).", upstream));
            score = score.saturating_sub(40);
        }
    }
    vec![Fix {
        title: format!("Undo your last command: `rgit {}` ({})", op.command, crate::workspace::undo::ago(op.time)),
        why,
        commands: vec![args(&["undo"])],
        score,
    }]
}

/// HEAD's last move was a commit or an amend: offer to take it back keeping the changes. A
/// commit the upstream already has gets `revert` instead (rewriting it would need a force-push).
// Used by `collect_fixes`.
fn suggest_uncommit(ctx: &OopsContext) -> Vec<Fix> {
    let (Some(head), Some(entry)) = (&ctx.head, ctx.head_reflog.first()) else { return Vec::new() };
    // On a detached HEAD, `suggest_detached` and `undo` cover it.
    if ctx.branch.is_none() || ctx.in_progress.is_some() || entry.new_hash != *head || ctx.now - entry.timestamp as i64 > RECENT {
        return Vec::new();
    }
    let pushed = ctx.upstream.as_ref().is_some_and(|(_, tip)| is_ancestor(head, tip));
    // `rgit undo` of that very commit does the same and is already suggested.
    if !pushed && clean_undo_target(ctx).is_some_and(|op| op.command.starts_with("commit") && op.after.refs.values().any(|h| h == head)) {
        return Vec::new();
    }
    if entry.message.starts_with("commit (amend)") && entry.old_hash != NULL_HASH {
        if pushed {
            return Vec::new();
        }
        return vec![Fix {
            title: format!("Undo the amend: get back {} \"{}\" as it was before", short(&entry.old_hash), subject(&entry.old_hash)),
            why: "The amended commit replaced it; this moves the branch back to the original and keeps the amended changes staged.".into(),
            commands: vec![args(&["reset", "--soft", &entry.old_hash[..12]])],
            score: 62,
        }];
    }
    let initial = entry.message.starts_with("commit (initial)");
    if !entry.message.starts_with("commit") || initial || entry.message.starts_with("commit (merge)") {
        return Vec::new();
    }
    if pushed {
        return vec![Fix {
            title: format!("Reverse the last commit {} \"{}\" with a new commit", short(head), subject(head)),
            why: format!("It's already on {}, so instead of rewriting history this adds a commit that undoes its changes.", ctx.upstream.as_ref().map(|u| u.0.as_str()).unwrap_or("the upstream")),
            commands: vec![args(&["revert", "--no-edit", "HEAD"])],
            score: 40,
        }];
    }
    vec![Fix {
        title: format!("Take back the last commit {} \"{}\" but keep its changes staged", short(head), subject(head)),
        why: "For a commit made too early or with the wrong message: the branch moves back one commit and nothing is lost.".into(),
        commands: vec![args(&["reset", "--soft", "HEAD~1"])],
        score: 58,
    }]
}

/// Detached HEAD: new commits there belong on a branch; otherwise offer the way back.
// Used by `collect_fixes`.
fn suggest_detached(ctx: &OopsContext) -> Vec<Fix> {
    if ctx.branch.is_some() {
        return Vec::new();
    }
    let Some(head) = &ctx.head else { return Vec::new() };
    let branches: Vec<&String> = ctx.ref_tips.iter().filter(|(n, _)| n.starts_with("refs/heads/")).map(|(_, t)| t).collect();
    let orphaned = !branches.iter().any(|tip| is_ancestor(head, tip));
    let previous = ctx.head_reflog.iter().find_map(|e| {
        let from = e.message.strip_prefix("checkout: moving from ")?.split(" to ").next()?;
        ctx.ref_tips.contains_key(&format!("refs/heads/{}", from)).then(|| from.to_string())
    });
    if orphaned {
        let base = branches.iter().filter(|tip| is_ancestor(tip, head)).count();
        let count = if base > 0 {
            branches.iter().filter(|tip| is_ancestor(tip, head)).map(|tip| commits_between(Some(tip), head)).min().unwrap_or(1)
        } else {
            commits_between(None, head)
        };
        let name = free_branch_name(ctx, &slug(&subject(head)));
        return vec![Fix {
            title: format!("Keep the {} you made on a detached HEAD on a new branch '{}'", plural(count, "commit", "commits"), name),
            why: "HEAD isn't on a branch, so switching away would leave these commits behind; this gives them a branch and puts you on it.".into(),
            commands: vec![args(&["switch", "-c", &name])],
            score: 75,
        }];
    }
    let Some(previous) = previous else { return Vec::new() };
    vec![Fix {
        title: format!("Go back to branch '{}' (HEAD is detached)", previous),
        why: format!("You're looking at {} directly, not at a branch; new commits here would belong to no branch.", short(head)),
        commands: vec![args(&["switch", &previous])],
        score: 45,
    }]
}

/// Recent commits on the default branch (main/master) that the upstream doesn't have: offer
/// to move them to a new branch and put the default branch back.
// Used by `collect_fixes`.
fn suggest_move_commits(ctx: &OopsContext) -> Vec<Fix> {
    let (Some(branch), Some(head)) = (&ctx.branch, &ctx.head) else { return Vec::new() };
    if !is_default_branch(branch) || ctx.in_progress.is_some() {
        return Vec::new();
    }
    let base = match &ctx.upstream {
        Some((_, tip)) => {
            if !is_ancestor(tip, head) || tip == head {
                return Vec::new();
            }
            tip.clone()
        }
        None => {
            // Without an upstream: the commits made in a row just now, on top of older history.
            let mut base = None;
            for e in &ctx.head_reflog {
                if ctx.now - e.timestamp as i64 > RECENT || !e.message.starts_with("commit") || e.message.starts_with("commit (initial)") || e.message.starts_with("commit (merge)") {
                    break;
                }
                base = Some(e.old_hash.clone());
            }
            match base {
                Some(b) if b != NULL_HASH && is_ancestor(&b, head) && b != *head => b,
                _ => return Vec::new(),
            }
        }
    };
    let count = commits_between(Some(&base), head);
    if count == 0 || count > 20 {
        return Vec::new();
    }
    let name = free_branch_name(ctx, &slug(&subject(head)));
    let mut commands = Vec::new();
    if ctx.dirty {
        commands.push(args(&["stash"]));
    }
    commands.push(args(&["branch", &name]));
    commands.push(args(&["reset", "--hard", &base[..12]]));
    commands.push(args(&["switch", &name]));
    if ctx.dirty {
        commands.push(args(&["stash", "pop"]));
    }
    let base_label = ctx.upstream.as_ref().map(|u| format!("{} ({})", u.0, short(&base))).unwrap_or_else(|| short(&base).to_string());
    vec![Fix {
        title: format!("Move your last {} off '{}' to a new branch '{}'", plural(count, "commit", "commits"), branch, name),
        why: format!(
            "Committed on '{}' instead of a topic branch? This puts the {} on '{}' and moves '{}' back to {}.{}",
            branch,
            if count == 1 { "commit" } else { "commits" },
            name,
            branch,
            base_label,
            if ctx.dirty { " Uncommitted changes are stashed first and come along." } else { "" }
        ),
        commands,
        score: if ctx.upstream.is_some() { 48 } else { 42 },
    }]
}

/// Branches deleted recently that don't exist any more: offer to recreate them where they were.
/// The operation log knows exactly what was deleted; HEAD's reflog catches branches deleted by
/// real git or before rgit kept a log (from the last time you left them).
// Used by `collect_fixes`.
fn suggest_restore_branches(ctx: &OopsContext) -> Vec<Fix> {
    let mut found: BTreeMap<String, (String, i64, bool)> = BTreeMap::new();
    let (done, _) = undo_stacks(&ctx.ops);
    let newest_op = done.last().copied();
    for op in ctx.ops.iter().rev().take(200) {
        if !matches!(op.kind, OpKind::Command) || !done.contains(&op.id) {
            continue;
        }
        for (name, hash) in &op.before.refs {
            let Some(b) = name.strip_prefix("refs/heads/") else { continue };
            if !op.after.refs.contains_key(name) && !ctx.ref_tips.contains_key(name) && !found.contains_key(b) {
                found.insert(b.to_string(), (hash.clone(), op.time, Some(op.id) == newest_op));
            }
        }
    }
    for e in &ctx.head_reflog {
        if ctx.now - e.timestamp as i64 > LOST_WINDOW {
            break;
        }
        let Some(rest) = e.message.strip_prefix("checkout: moving from ") else { continue };
        let Some(from) = rest.split(" to ").next() else { continue };
        let is_hash = from.len() >= 7 && from.chars().all(|c| c.is_ascii_hexdigit());
        if is_hash || found.contains_key(from) || ctx.ref_tips.contains_key(&format!("refs/heads/{}", from)) || e.old_hash == NULL_HASH {
            continue;
        }
        // Only when its commits are on no other ref: a renamed or merged branch lost nothing.
        if refs::is_valid_branch_name(from) && !reachable_from_any_ref(ctx, &e.old_hash) {
            found.insert(from.to_string(), (e.old_hash.clone(), e.timestamp as i64, false));
        }
    }
    found
        .into_iter()
        .map(|(name, (hash, time, newest))| Fix {
            title: format!("Bring back the deleted branch '{}' (it was at {} \"{}\")", name, short(&hash), subject(&hash)),
            why: format!(
                "It was deleted {}{}; this recreates it at the same commit.",
                crate::workspace::undo::ago(time),
                if reachable_from_any_ref(ctx, &hash) { "" } else { " and its last commits are on no other branch" }
            ),
            commands: vec![args(&["branch", &name, &hash[..12]])],
            score: if newest { 60 } else if ctx.now - time < RECENT { 50 } else { 30 },
        })
        .collect()
}

/// Commits HEAD left behind (reset --hard, amend, rebase, undo) that no ref reaches any more:
/// offer to put a branch on the newest of them, so nothing is lost when the reflog expires.
// Used by `collect_fixes`.
fn suggest_lost_commits(ctx: &OopsContext) -> Vec<Fix> {
    let Some(head) = &ctx.head else { return Vec::new() };
    // What the latest undo took back is better brought back with `rgit redo`.
    let redo_hashes: BTreeSet<&String> = match ctx.ops.last() {
        Some(op) if matches!(op.kind, OpKind::Undo { .. }) => op.before.refs.values().collect(),
        _ => BTreeSet::new(),
    };
    for e in &ctx.head_reflog {
        if ctx.now - e.timestamp as i64 > LOST_WINDOW {
            break;
        }
        let lost = &e.old_hash;
        if lost == NULL_HASH || lost == head || is_ancestor(lost, head) || redo_hashes.contains(lost) || reachable_from_any_ref(ctx, lost) {
            continue;
        }
        if crate::core::object_store::read_object(lost).is_err() {
            continue;
        }
        // Undoing the last command brings them back already (and is suggested).
        if clean_undo_target(ctx).is_some_and(|op| op.before.refs.values().any(|h| h == lost)) {
            return Vec::new();
        }
        let count = commits_between(Some(head), lost).max(1);
        let name = free_branch_name(ctx, &format!("rescued-{}", short(lost)));
        // The command that left them behind: from the operation log, else the reflog message.
        let logged = ctx.ops.iter().rev().find(|o| o.before.refs.values().any(|h| h == lost) && !o.after.refs.values().any(|h| h == lost));
        let cause = match logged {
            Some(op) => format!("`rgit {}`", op.command),
            None => match e.message.split(':').next().map(str::trim) {
                Some(m) if !m.is_empty() && m != "update" => format!("`{}`", m),
                _ => String::new(),
            },
        };
        return vec![Fix {
            title: format!("Recover {} that no branch has any more (newest: {} \"{}\")", plural(count, "commit", "commits"), short(lost), subject(lost)),
            why: format!(
                "HEAD moved away from them {}{}, and no branch points at them (a reset, an amend, a rebase or a deleted branch); they only survive in the reflog until it expires. This puts a branch '{}' on them.",
                if cause.is_empty() { String::new() } else { format!("with {} ", cause) },
                crate::workspace::undo::ago(e.timestamp as i64),
                name
            ),
            commands: vec![args(&["branch", &name, &lost[..12]])],
            score: if ctx.now - (e.timestamp as i64) < RECENT { 52 } else { 35 },
        }];
    }
    Vec::new()
}
