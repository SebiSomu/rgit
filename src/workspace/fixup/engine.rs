//! Helpers of `rgit fixup`: the commits it can rewrite, the pushed-commit check, the picker and
//! the rebase todo that folds the fixup commit into its target.

use std::io::{BufRead, Write};

use anyhow::Result;

use crate::core::commit::{is_reachable, parse_commit_object};
use crate::core::refs;
use crate::merge::rebase::types::{Action, TodoItem};
use crate::workspace::fixup::types::Candidate;

/// How many commits the picker offers.
pub const PICKER_LIMIT: usize = 10;

/// The first remote-tracking branch (`origin/main`) that contains `commit`, if any.
// Used by `candidates` and the `fixup` command's pushed-commit check.
pub fn pushed_on(commit: &str) -> Result<Option<String>> {
    for (name, tip) in refs::list_all_refs() {
        let Some(short) = name.strip_prefix("refs/remotes/") else { continue };
        if short.ends_with("/HEAD") {
            continue;
        }
        if is_reachable(&tip, commit)? {
            return Ok(Some(short.to_string()));
        }
    }
    Ok(None)
}

/// The commits a fixup can target: HEAD and its first parents, newest first, up to `limit`,
/// stopping before a merge commit (the replay can't cross one) and before the root commit
/// (nothing to replay it onto) unless the root is HEAD itself.
// Used by the `fixup` command's picker.
pub fn candidates(head: &str, limit: usize) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    let mut current = head.to_string();
    while out.len() < limit {
        let commit = parse_commit_object(&current)?;
        if commit.parents.len() > 1 || (commit.parents.is_empty() && current != head) {
            break;
        }
        out.push(Candidate {
            hash: current.clone(),
            subject: commit.message.lines().next().unwrap_or("").to_string(),
            pushed_on: pushed_on(&current)?,
        });
        match commit.parents.first() {
            Some(parent) => current = parent.clone(),
            None => break,
        }
    }
    Ok(out)
}

/// Prints the numbered list and reads a number from standard input. None when the answer is
/// empty (or there is no input): the user cancelled.
// Used by the `fixup` command when no commit is given.
pub fn pick_interactively(list: &[Candidate]) -> Result<Option<String>> {
    println!("Fix up which commit?");
    for (i, c) in list.iter().enumerate() {
        let pushed = c.pushed_on.as_ref().map(|r| format!("  [pushed: {}]", r)).unwrap_or_default();
        println!("  {:>2}  {}  {}{}", i + 1, &c.hash[..7], c.subject, pushed);
    }
    print!("Number (Enter cancels): ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    let answer = answer.trim();
    println!();
    if answer.is_empty() {
        return Ok(None);
    }
    match answer.parse::<usize>() {
        Ok(n) if (1..=list.len()).contains(&n) => Ok(Some(list[n - 1].hash.clone())),
        _ => crate::ui::die_fatal(format!("'{}' is not one of 1-{}", answer, list.len())),
    }
}

/// The rebase todo that folds `fixup_commit` (on top of HEAD) into `target`: every commit from
/// `target` up to the old HEAD as `pick`, with `fixup <fixup_commit>` right after `target`.
/// Fails on a merge in between.
// Used by the `fixup` command.
pub fn fixup_todo(fixup_commit: &str, target: &str, target_parent: &str) -> Result<Vec<TodoItem>> {
    let commits = crate::merge::rebase::engine::collect_commits_to_replay(fixup_commit, target_parent)?;
    let mut items = Vec::new();
    for hash in commits.into_iter().filter(|h| h != fixup_commit) {
        let is_target = hash == target;
        let subject = crate::merge::rebase::engine::subject_of(&hash)?;
        items.push(TodoItem { action: Action::Pick, hash, subject });
        if is_target {
            let subject = crate::merge::rebase::engine::subject_of(fixup_commit)?;
            items.push(TodoItem { action: Action::Fixup, hash: fixup_commit.to_string(), subject });
        }
    }
    Ok(items)
}

/// Whether the index differs from HEAD's tree (something is staged).
// Used by the `fixup` command.
pub fn has_staged_changes(head: &str) -> Result<bool> {
    let entries = crate::core::index::read_index().unwrap_or_default();
    let index_tree = crate::core::object_store::write_tree_from_index_prefix(&entries, "")?;
    Ok(index_tree != crate::core::object_store::tree_hash_of_commit(head)?)
}
