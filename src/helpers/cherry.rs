use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use sha1::{Digest, Sha1};
use crate::helpers::config::get_config_value;
use crate::helpers::format_patch::{parse_commit_object, split_diff_lines, unified_hunks_with};
use crate::helpers::rev_list::run_rev_list_walk;
use crate::helpers::{flatten_tree, read_object};
use crate::objects::{ParsedCommit, RevListOptions, RevListRoots};

/// Finds the upstream of the current branch from `branch.<name>.remote` and
/// `branch.<name>.merge`: `refs/heads/<merge>` for a remote of `.`, otherwise the
/// remote-tracking ref `refs/remotes/<remote>/<merge>`. Returns `None` when HEAD is detached
/// or the branch has no upstream configured.
// Used by `cherry` when no `<upstream>` is given.
pub fn resolve_cherry_upstream() -> anyhow::Result<Option<String>> {
    let branch = match crate::refs::resolve_head()? {
        crate::refs::HeadState::Branch(b) => b,
        crate::refs::HeadState::Detached(_) => return Ok(None),
    };
    let (Some(remote), Some(merge)) = (
        get_config_value(&format!("branch.{}.remote", branch))?,
        get_config_value(&format!("branch.{}.merge", branch))?,
    ) else {
        return Ok(None);
    };
    let merge = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    Ok(Some(if remote == "." { format!("refs/heads/{}", merge) } else { format!("refs/remotes/{}/{}", remote, merge) }))
}

/// Lists the non-merge commits reachable from `include` but not from any of `exclude`, in
/// git's default revision-walk order (newest first by committer date).
// Used by `cherry`.
pub fn walk_non_merge_commits(include: &str, exclude: &[&str]) -> anyhow::Result<Vec<ParsedCommit>> {
    let opts = RevListOptions { max_parents: Some(1), ..Default::default() };
    let mut roots = RevListRoots::default();
    roots.pending.push((include.to_string(), false, false));
    for hash in exclude {
        roots.bottoms.insert(hash.to_string());
        roots.pending.push((hash.to_string(), true, false));
    }
    let (walked, state) = run_rev_list_walk(&opts, &roots, &|_, parents| parents <= 1)?;
    Ok(walked
        .into_iter()
        .filter(|w| !w.hidden && !state.uninteresting.contains(&w.commit.hash) && w.parents.len() <= 1)
        .map(|w| w.commit)
        .collect())
}

/// Computes a commit's patch ID the way git does for `cherry` (equal to
/// `git patch-id --stable`): every path changed against the first parent (or the empty tree)
/// is hashed separately, without line numbers or whitespace (git's space, tab, CR, LF), as its `diff --git` and mode
/// header lines, then either both blob hashes (binary files) or the `---`/`+++` lines and
/// hunk bodies with 3 lines of context (no hunk headers, no indent heuristic). The per-file
/// SHA-1s are summed bytewise with carry, so file order doesn't matter.
// Used by `cherry`.
pub fn commit_patch_id(commit: &ParsedCommit) -> anyhow::Result<[u8; 20]> {
    let mut old_tree = BTreeMap::new();
    if let Some(parent) = commit.parents.first() {
        flatten_tree(&parse_commit_object(parent)?.tree, "", &mut old_tree)?;
    }
    let mut new_tree = BTreeMap::new();
    flatten_tree(&commit.tree, "", &mut new_tree)?;

    let mut id = [0u8; 20];
    let paths: BTreeSet<&String> = old_tree.keys().chain(new_tree.keys()).collect();
    for path in paths {
        let old = old_tree.get(path).copied();
        let new = new_tree.get(path).copied();
        if old == new {
            continue;
        }
        let file_hash = file_patch_id(path, old, new)?;
        let mut carry: u16 = 0;
        for (byte, add) in id.iter_mut().zip(file_hash) {
            carry += *byte as u16 + add as u16;
            *byte = carry as u8;
            carry >>= 8;
        }
    }
    Ok(id)
}

fn file_patch_id(path: &str, old: Option<([u8; 20], u32)>, new: Option<([u8; 20], u32)>) -> anyhow::Result<[u8; 20]> {
    let load = |entry: Option<([u8; 20], u32)>| -> anyhow::Result<Vec<u8>> {
        match entry {
            Some((hash, _)) => Ok(read_object(&hex::encode(hash))?.1),
            None => Ok(Vec::new()),
        }
    };
    let path = strip_whitespace(path);
    let mut hasher = Sha1::new();
    hasher.update(format!("diff--gita/{}b/{}", path, path));
    match (old, new) {
        (None, Some((_, mode))) => hasher.update(format!("newfilemode{:06o}", mode)),
        (Some((_, mode)), None) => hasher.update(format!("deletedfilemode{:06o}", mode)),
        (Some((_, old_mode)), Some((_, new_mode))) if old_mode != new_mode => {
            hasher.update(format!("oldmode{:06o}newmode{:06o}", old_mode, new_mode))
        }
        _ => {}
    }

    let (old_bytes, new_bytes) = (load(old)?, load(new)?);
    let is_binary = |b: &[u8]| b[..b.len().min(8000)].contains(&0);
    if is_binary(&old_bytes) || is_binary(&new_bytes) {
        let hex_of = |e: Option<([u8; 20], u32)>| e.map(|(h, _)| hex::encode(h)).unwrap_or_else(|| "0".repeat(40));
        hasher.update(hex_of(old));
        hasher.update(hex_of(new));
        return Ok(hasher.finalize().into());
    }

    hasher.update(match (old, new) {
        (None, _) => format!("---/dev/null+++b/{}", path),
        (_, None) => format!("---a/{}+++/dev/null", path),
        _ => format!("---a/{}+++b/{}", path, path),
    });
    let (hunks, _, _) = unified_hunks_with(&split_diff_lines(&old_bytes), &split_diff_lines(&new_bytes), false);
    for line in hunks.lines().filter(|l| !l.starts_with("@@") && !l.starts_with("\\ ")) {
        hasher.update(strip_whitespace(line));
    }
    Ok(hasher.finalize().into())
}

fn strip_whitespace(text: &str) -> String {
    text.chars().filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r')).collect()
}

/// Shortens a hash to at least `min_len` characters (clamped to 4..=40), lengthening it until
/// no other loose object shares the prefix.
// Used by `cherry --abbrev`.
pub fn abbreviate_hash_min(hash: &str, min_len: usize) -> String {
    let others: Vec<String> = fs::read_dir(crate::repo::path(format!(".git/objects/{}", &hash[..2])))
        .map(|entries| entries.flatten().map(|e| format!("{}{}", &hash[..2], e.file_name().to_string_lossy())).filter(|h| h != hash).collect())
        .unwrap_or_default();
    let mut len = min_len.clamp(4, hash.len());
    while len < hash.len() && others.iter().any(|o| o.starts_with(&hash[..len])) {
        len += 1;
    }
    hash[..len].to_string()
}
