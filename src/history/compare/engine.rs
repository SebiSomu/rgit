//! Helpers of `rgit compare`: the commits only one side has, which of them the other side has
//! as a copy (same patch), and the files that differ with how each side changed them.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use anyhow::Result;

use crate::core::object_store::{flatten_tree, tree_hash_of_commit};
use crate::core::types::ParsedCommit;
use crate::history::compare::types::{Change, FileDiff};

type Tree = BTreeMap<String, ([u8; 20], u32)>;

/// A commit's flattened tree (empty for None).
// Used by `file_differences`.
fn tree(commit: Option<&str>) -> Result<Tree> {
    let mut out = Tree::new();
    if let Some(c) = commit {
        flatten_tree(&tree_hash_of_commit(c)?, "", &mut out)?;
    }
    Ok(out)
}

/// The commits reachable from `tip` but not from `other`, newest first (merges included).
// Used by the `compare` command.
pub fn only_on(tip: &str, other: &str) -> Result<Vec<ParsedCommit>> {
    let opts = crate::history::rev_list::engine::parse_revision_args(&[tip.to_string(), format!("^{}", other)])?;
    let (walked, _) = crate::history::rev_list::engine::select_rev_list_commits(&opts)?;
    Ok(walked.into_iter().map(|w| w.commit).collect())
}

/// How many of `mine` (non-merge commits) have the same patch as one of `theirs` (a copy:
/// cherry-picked, rebased or applied on the other side).
// Used by the `compare` command.
pub fn copies(mine: &[ParsedCommit], theirs: &[ParsedCommit]) -> Result<usize> {
    let mut ids: HashSet<[u8; 20]> = HashSet::new();
    for c in theirs.iter().filter(|c| c.parents.len() == 1) {
        ids.insert(crate::history::cherry::engine::commit_patch_id(c)?);
    }
    let mut n = 0;
    for c in mine.iter().filter(|c| c.parents.len() == 1) {
        if ids.contains(&crate::history::cherry::engine::commit_patch_id(c)?) {
            n += 1;
        }
    }
    Ok(n)
}

/// How a file went from `base` to `side`.
// Used by `file_differences`.
fn side_change(base: Option<&([u8; 20], u32)>, side: Option<&([u8; 20], u32)>) -> Change {
    match (base, side) {
        (a, b) if a == b => Change::Same,
        (None, Some(_)) => Change::Added,
        (Some(_), None) => Change::Deleted,
        _ => Change::Changed,
    }
}

/// The files whose content differs between `left` and `right`, with how each side changed
/// them since `base` (everything counts as added on its side without a base).
// Used by the `compare` command.
pub fn file_differences(left: &str, right: &str, base: Option<&str>) -> Result<Vec<FileDiff>> {
    let (l, r, b) = (tree(Some(left))?, tree(Some(right))?, tree(base)?);
    let paths: BTreeSet<&String> = l.keys().chain(r.keys()).filter(|p| l.get(*p) != r.get(*p)).collect();
    Ok(paths.into_iter().map(|p| FileDiff { path: p.clone(), left: side_change(b.get(p), l.get(p)), right: side_change(b.get(p), r.get(p)) }).collect())
}

/// The files a merge of the two tips would leave conflicted (an in-memory three-way merge from
/// their merge base, or a virtual one for criss-cross histories).
// Used by the `compare` command.
pub fn merge_conflicts(left: &str, right: &str) -> Result<Vec<String>> {
    let (base, _) = crate::merge::merge_tree::engine::merge_base_tree(left, right, true)?;
    let merged = crate::merge::merge_tree::engine::merge_trees_in_memory(&base, &tree(Some(left))?, &tree(Some(right))?, "ours", "theirs", 7)?;
    Ok(merged.conflicted.keys().cloned().collect())
}
