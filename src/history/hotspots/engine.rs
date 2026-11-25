//! Helpers of `rgit hotspots`: one walk over the history collecting, per file, the commits,
//! lines and authors that changed it, and which files changed together.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::core::object_store::{flatten_tree, read_object};
use crate::diff::types::DiffOp;
use crate::history::hotspots::types::Hotspots;

/// Commits that change more files than this are left out of the coupling count (a reformat or
/// a mass rename says nothing about which files belong together).
pub const COUPLING_MAX_FILES: usize = 30;

type Tree = BTreeMap<String, ([u8; 20], u32)>;

/// Lines added and removed between two blobs (0 / 0 for binary content).
// Used by `collect` and `rgit stats`.
pub fn line_changes(old: Option<[u8; 20]>, new: Option<[u8; 20]>) -> Result<(usize, usize)> {
    let read = |h: Option<[u8; 20]>| -> Result<Option<String>> {
        match h {
            None => Ok(Some(String::new())),
            Some(h) => {
                let content = read_object(&hex::encode(h))?.1;
                Ok(if content.contains(&0) { None } else { Some(String::from_utf8_lossy(&content).to_string()) })
            }
        }
    };
    let (Some(old), Some(new)) = (read(old)?, read(new)?) else { return Ok((0, 0)) };
    let (o, n): (Vec<&str>, Vec<&str>) = (old.split_inclusive('\n').collect(), new.split_inclusive('\n').collect());
    let (mut added, mut removed) = (0, 0);
    for op in crate::diff::myers::myers_diff(&o, &n) {
        match op {
            DiffOp::Insert(_) => added += 1,
            DiffOp::Delete(_) => removed += 1,
            DiffOp::Keep(_) => {}
        }
    }
    Ok((added, removed))
}

/// Walks the non-merge commits reachable from `head` (committed at or after `since`), each
/// against its first parent, keeping files under `paths` (all when empty) that still exist at
/// HEAD.
// Used by the `hotspots` command.
pub fn collect(head: &str, paths: &[String], since: Option<i64>) -> Result<Hotspots> {
    let mut head_tree = Tree::new();
    flatten_tree(&crate::core::object_store::tree_hash_of_commit(head)?, "", &mut head_tree)?;
    let specs = crate::workspace::ls_files::engine::normalize_pathspecs(paths);
    let wanted = |p: &str| head_tree.contains_key(p) && (specs.is_empty() || crate::workspace::ls_files::engine::matches_ls_files_pathspec(p, &specs));

    let opts = crate::history::rev_list::engine::parse_revision_args(&[head.to_string()])?;
    let (walked, _) = crate::history::rev_list::engine::select_rev_list_commits(&opts)?;
    let mut out = Hotspots::default();
    for w in walked {
        let commit = &w.commit;
        if commit.parents.len() > 1 || since.is_some_and(|s| commit.committer.timestamp < s) {
            continue;
        }
        let mut old_tree = Tree::new();
        if let Some(parent) = commit.parents.first() {
            flatten_tree(&crate::core::object_store::tree_hash_of_commit(parent)?, "", &mut old_tree)?;
        }
        let mut new_tree = Tree::new();
        flatten_tree(&commit.tree, "", &mut new_tree)?;
        let changed: BTreeSet<&String> = old_tree.keys().chain(new_tree.keys()).filter(|p| old_tree.get(*p) != new_tree.get(*p)).collect();
        let kept: Vec<&String> = changed.iter().copied().filter(|p| wanted(p)).collect();
        if kept.is_empty() {
            continue;
        }
        out.commits += 1;
        for path in &kept {
            let (added, removed) = line_changes(old_tree.get(*path).map(|e| e.0), new_tree.get(*path).map(|e| e.0))?;
            let stats = out.files.entry((*path).clone()).or_default();
            stats.commits += 1;
            stats.added += added;
            stats.removed += removed;
            *stats.authors.entry(commit.author.name.clone()).or_default() += 1;
        }
        if changed.len() > COUPLING_MAX_FILES {
            out.bulk += 1;
        } else {
            for (i, a) in kept.iter().enumerate() {
                for b in &kept[i + 1..] {
                    *out.pairs.entry(((*a).clone(), (*b).clone())).or_default() += 1;
                }
            }
        }
    }
    Ok(out)
}
