//! Helpers of `rgit pr-summary` (and `rgit open pr`): the branch a pull request goes into,
//! the files it changes with their lines, and a title made from a branch name.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit};
use crate::remote::pr_summary::types::ChangedFile;

/// The default base of a pull request: the remote's HEAD (`refs/remotes/<remote>/HEAD`), else
/// `<remote>/main` or `<remote>/master`, else a local `main` or `master`. As (name, commit).
// Used by `pr_summary` and `rgit open pr`.
pub fn default_base(remote: &str) -> Option<(String, String)> {
    let read = |r: &str| crate::core::refs::read_ref(r).ok().flatten();
    if let Ok(text) = std::fs::read_to_string(crate::core::repo::path(format!(".git/refs/remotes/{}/HEAD", remote)))
        && let Some(target) = text.trim().strip_prefix("ref: refs/remotes/")
        && let Some(hash) = read(&format!("refs/remotes/{}", target))
    {
        return Some((target.to_string(), hash));
    }
    for name in ["main", "master"] {
        if let Some(hash) = read(&format!("refs/remotes/{}/{}", remote, name)) {
            return Some((format!("{}/{}", remote, name), hash));
        }
    }
    for name in ["main", "master"] {
        if let Some(hash) = read(&format!("refs/heads/{}", name)) {
            return Some((name.to_string(), hash));
        }
    }
    None
}

/// The files that differ between `base` and `head` (their trees), with line counts.
// Used by `pr_summary`.
pub fn changed_files(base: &str, head: &str) -> Result<Vec<ChangedFile>> {
    let tree = |c: &str| -> Result<BTreeMap<String, ([u8; 20], u32)>> {
        let mut t = BTreeMap::new();
        flatten_tree(&tree_hash_of_commit(c)?, "", &mut t)?;
        Ok(t)
    };
    let (old, new) = (tree(base)?, tree(head)?);
    let paths: BTreeSet<&String> = old.keys().chain(new.keys()).filter(|p| old.get(*p) != new.get(*p)).collect();
    let mut out = Vec::new();
    for p in paths {
        let (o, n) = (old.get(p).map(|e| e.0), new.get(p).map(|e| e.0));
        let binary = [o, n].iter().flatten().any(|h| read_object(&hex::encode(h)).map(|(_, c)| c.contains(&0)).unwrap_or(false));
        let (added, removed) = crate::history::hotspots::engine::line_changes(o, n)?;
        out.push(ChangedFile { path: p.clone(), new: o.is_none(), deleted: n.is_none(), binary, added, removed });
    }
    Ok(out)
}

/// A title from a branch name: `feature/add-parser` -> `Add parser`.
// Used by `pr_summary`.
pub fn title_from_branch(branch: &str) -> String {
    let last = branch.rsplit('/').next().unwrap_or(branch);
    let words = last.replace(['-', '_'], " ");
    let words = words.trim();
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => branch.to_string(),
    }
}
