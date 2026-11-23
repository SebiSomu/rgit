pub mod cli;
pub mod engine;
pub mod types;

use std::collections::BTreeMap;

use anyhow::Result;

use crate::workspace::wip::engine::{branches_with_saves, parse_position, read_stack, remove_entry, require_branch, set_top, wip_ref};
use crate::workspace::wip::types::WipEntry;

/// `rgit wip [<message>]`: saves everything uncommitted (staged and unstaged changes and the
/// untracked files, not ignored ones) as a WIP commit on `refs/rgit/wip/<branch>` and leaves the
/// working tree clean at HEAD. Saves stack per branch.
pub fn save(message: Option<String>) -> Result<()> {
    if crate::merge::merge::engine::report_needs_merge() {
        crate::ui::exit(1);
    }
    let branch = require_branch()?;
    let Some(snap) = crate::workspace::stash::capture_snapshot(Some(false))? else {
        println!("No local changes to save");
        return Ok(());
    };
    // Always four parents' worth of shape: an empty untracked commit keeps the stack link fourth.
    let untracked = match &snap.untracked_commit {
        Some(u) => u.clone(),
        None => {
            let empty = crate::workspace::state_files::build_tree_from_map(&BTreeMap::new())?;
            let label = format!("untracked files on {}: {} {}", branch, &snap.head[..7], snap.head_subject);
            crate::workspace::state_files::build_stash_commit(&empty, &[], &label)?
        }
    };
    let text = match &message {
        Some(m) => format!("WIP on {}: {}", branch, m),
        None => format!("WIP on {}: {} {}", branch, &snap.head[..7], snap.head_subject),
    };
    let below = crate::core::refs::read_ref(&wip_ref(&branch))?;
    let mut parents = vec![snap.head.as_str(), snap.index_commit.as_str(), untracked.as_str()];
    parents.extend(below.as_deref());
    let commit = crate::workspace::state_files::build_stash_commit(&snap.working_tree, &parents, &text)?;
    set_top(&branch, Some(&commit))?;
    crate::workspace::stash::clear_saved_changes(&snap)?;

    println!("Saved wip@{{0}} ({})", text);
    println!("The working tree is clean; `rgit wip --restore` brings the changes back.");
    Ok(())
}

/// `rgit wip --list [--all]`: the current branch's saves, newest first (every branch's with
/// `--all` or on a detached HEAD).
pub fn list(all: bool) -> Result<()> {
    let current = crate::workspace::wip::engine::current_branch()?;
    match current.filter(|_| !all) {
        Some(branch) => {
            let stack = read_stack(&branch)?;
            if stack.is_empty() {
                println!("No WIP saved on '{}'.", branch);
                hint_other_branches(&branch);
            }
            for (i, entry) in stack.iter().enumerate() {
                println!("{}", line(i, entry));
            }
        }
        None => {
            let branches = branches_with_saves();
            if branches.is_empty() {
                println!("No WIP saved on any branch.");
            }
            for branch in branches {
                println!("{}:", branch);
                for (i, entry) in read_stack(&branch)?.iter().enumerate() {
                    println!("  {}", line(i, entry));
                }
            }
        }
    }
    Ok(())
}

/// One `--list` line: `wip@{0}: WIP on main: parser (5 minutes ago)`.
// Used by `list`.
fn line(i: usize, entry: &WipEntry) -> String {
    format!("wip@{{{}}}: {} ({})", i, entry.message, crate::workspace::undo::ago(entry.time))
}

/// Mentions the branches that do have saves, when the current one has none.
// Used by `list`, `restore` and `drop`.
fn hint_other_branches(branch: &str) {
    let others: Vec<String> = branches_with_saves().into_iter().filter(|b| b != branch).collect();
    if !others.is_empty() {
        println!("(Saved on other branches: {}; `rgit wip --list --all` shows them.)", others.join(", "));
    }
}

/// Finds save `raw` on the current branch, failing clearly when there is none.
// Used by `restore` and `drop`.
fn pick(raw: &str) -> Result<(String, usize, Vec<WipEntry>)> {
    let branch = require_branch()?;
    let n = parse_position(Some(raw))?;
    let stack = read_stack(&branch)?;
    if stack.is_empty() {
        println!("No WIP saved on '{}'.", branch);
        hint_other_branches(&branch);
        crate::ui::exit(1);
    }
    if n >= stack.len() {
        crate::ui::die_fatal(format!("wip@{{{}}} does not exist ('{}' has {} WIP save{})", n, branch, stack.len(), if stack.len() == 1 { "" } else { "s" }));
    }
    Ok((branch, n, stack))
}

/// `rgit wip --restore [<n>]`: puts save `n` back (staged changes staged again, untracked files
/// untracked) and takes it off the stack. If HEAD moved since the save, the changes are merged
/// onto it; on conflicts the save is kept and the exit code is 1.
pub fn restore(raw: &str) -> Result<()> {
    let (branch, n, stack) = pick(raw)?;
    let entry = &stack[n];
    crate::workspace::checkout::ensure_working_tree_clean("restore WIP")?;
    let (w_tree, i_tree) = crate::workspace::state_files::load_commit_trees(&entry.commit)?
        .ok_or_else(|| anyhow::anyhow!("fatal: wip@{{{}}} is not a WIP save (corrupted WIP stack?)", n))?;
    if !crate::workspace::state_files::apply_stash_trees(&entry.commit, &w_tree, &i_tree, "WIP changes", "restore WIP")? {
        eprintln!("The WIP save is kept: resolve the conflicts, then `rgit wip --drop{}`.", if n == 0 { String::new() } else { format!(" {}", n) });
        crate::ui::exit(1);
    }
    let removed = remove_entry(&branch, n)?;
    println!("Restored wip@{{{}}} ({})", n, removed.message);
    Ok(())
}

/// `rgit wip --drop [<n>]`: throws save `n` away (`rgit undo` brings it back).
pub fn drop(raw: &str) -> Result<()> {
    let (branch, n, _) = pick(raw)?;
    let removed = remove_entry(&branch, n)?;
    println!("Dropped wip@{{{}}} ({})", n, removed.message);
    Ok(())
}
