//! Helpers of `rgit move-commit`: resolving the commits to move against the current branch's
//! first-parent line, and replaying commits in memory onto a new base.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::core::commit::{collect_ancestors, current_signature, parse_commit_object, write_commit_object};
use crate::core::object_store::flatten_tree;
use crate::merge::move_commit::types::Replayed;

/// HEAD's first-parent line, newest first, up to (not including) the first merge commit or
/// the root commit's parent.
// Used by `select_commits`.
fn first_parent_line(head: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut current = Some(head.to_string());
    while let Some(hash) = current {
        let commit = parse_commit_object(&hash)?;
        if commit.parents.len() > 1 {
            break;
        }
        current = commit.parents.first().cloned();
        out.push(hash);
    }
    Ok(out)
}

/// The commits named by `specs` (revisions or `A..B` ranges), oldest first, plus the commits
/// from the oldest of them up to HEAD, oldest first. Every one must be on HEAD's first-parent
/// line after the last merge; the root commit can't be moved.
// Used by the `move-commit` command.
pub fn select_commits(specs: &[String], head: &str) -> Result<(Vec<String>, Vec<String>)> {
    let resolve = |rev: &str| -> Result<String> {
        crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", rev)).map_err(|_| anyhow::anyhow!("'{}' is not a commit", rev))
    };
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    for spec in specs {
        match spec.split_once("..") {
            Some((from, to)) => {
                let from = resolve(if from.is_empty() { "HEAD" } else { from })?;
                let to = resolve(if to.is_empty() { "HEAD" } else { to })?;
                let excluded = collect_ancestors(&from)?;
                let range: Vec<String> = collect_ancestors(&to)?.into_iter().filter(|c| !excluded.contains(c)).collect();
                if range.is_empty() {
                    anyhow::bail!("the range '{}' has no commits", spec);
                }
                wanted.extend(range);
            }
            None => {
                wanted.insert(resolve(spec)?);
            }
        }
    }
    let line = first_parent_line(head)?;
    for hash in &wanted {
        match line.iter().position(|c| c == hash) {
            None => anyhow::bail!("{} is not on the current branch's line of commits (after its last merge)", &hash[..7]),
            Some(i) if parse_commit_object(&line[i])?.parents.is_empty() => anyhow::bail!("{} is the root commit; it can't be moved", &hash[..7]),
            Some(_) => {}
        }
    }
    let oldest = line.iter().rposition(|c| wanted.contains(c)).expect("checked above");
    let mut chain: Vec<String> = line[..=oldest].to_vec();
    chain.reverse();
    let moved: Vec<String> = chain.iter().filter(|c| wanted.contains(*c)).cloned().collect();
    Ok((moved, chain))
}

/// Re-creates `commits` (oldest first, each a non-merge commit) on top of `onto`, entirely in
/// the object store: each is a three-way merge (base: its parent's tree, ours: the new tip,
/// theirs: the commit), keeping author and message; a commit whose parent is already the tip
/// is kept as it is. Stops at the first conflict.
// Used by the `move-commit` command, for the target branch and for the current one.
pub fn replay_in_memory(commits: &[String], onto: &str) -> Result<Replayed> {
    let tree_of = |commit: &str| -> Result<BTreeMap<String, ([u8; 20], u32)>> {
        let mut tree = BTreeMap::new();
        flatten_tree(&parse_commit_object(commit)?.tree, "", &mut tree)?;
        Ok(tree)
    };
    let committer = current_signature()?;
    let mut tip = onto.to_string();
    for hash in commits {
        let commit = parse_commit_object(hash)?;
        // Still on its own parent: nothing to redo, keep the commit as it is.
        if commit.parents[0] == tip {
            tip = hash.clone();
            continue;
        }
        let base = tree_of(&commit.parents[0])?;
        let merged = crate::merge::merge_tree::engine::merge_trees_in_memory(&base, &tree_of(&tip)?, &tree_of(hash)?, "HEAD", &hash[..7], 7)?;
        if !merged.conflicted.is_empty() {
            return Ok(Replayed::Conflict { commit: hash.clone(), paths: merged.conflicted.keys().cloned().collect() });
        }
        let tree = if merged.tree.is_empty() { crate::core::object_store::write_object("tree", b"")? } else { crate::workspace::state_files::build_tree_from_map(&merged.tree)? };
        tip = write_commit_object(&tree, std::slice::from_ref(&tip), &commit.author, &committer, &commit.message)?;
    }
    Ok(Replayed::Done(tip))
}
