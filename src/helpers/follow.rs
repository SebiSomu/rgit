//! `log --follow`: the per-commit diff of one path, with git's rename/copy detection for the
//! commit that created it (`try_to_follow_renames` in git's tree-diff.c, scored like
//! `diffcore-rename.c` / `diffcore-delta.c`), and rendering of that diff in `log`'s formats.

use std::collections::{BTreeMap, BTreeSet};

use crate::helpers::objects::flatten_tree;
use crate::helpers::format_patch::{diff_file_contents, format_diffstat, parse_commit_object};
use crate::helpers::log::binary_patch_as_notice;
use crate::helpers::ls_files::{matches_ls_files_pathspec, normalize_pathspecs};
use crate::helpers::objects::read_object;
use crate::helpers::rename::{estimate_similarity, span_hash, MAX_SCORE};
use crate::objects::{DiffStatEntry, FollowPair, LogOptions, ParsedCommit};

/// git's default minimum rename score (50%).
const MIN_SCORE: u64 = 30000;

type Tree = BTreeMap<String, ([u8; 20], u32)>;

/// The diff `log --follow <path>` shows for one non-merge commit, against its first parent (the
/// empty tree for a root commit), limited to `path`. When the commit creates the path, the whole
/// trees are searched for a rename or copy source (any file of the parent can be one, as with
/// `--find-copies-harder`); if one is found the diff becomes that single pair and the source path
/// is returned, for the rest of the walk to follow. Like git, a creation keeps only one pair.
// Used by `log --follow`.
pub fn follow_commit_pairs(commit: &ParsedCommit, path: &str) -> anyhow::Result<(Vec<FollowPair>, Option<String>)> {
    let mut old_tree = Tree::new();
    if let Some(parent) = commit.parents.first() {
        flatten_tree(&parse_commit_object(parent)?.tree, "", &mut old_tree)?;
    }
    let mut new_tree = Tree::new();
    flatten_tree(&commit.tree, "", &mut new_tree)?;

    let specs = normalize_pathspecs(&[path.to_string()]);
    let paths: BTreeSet<&String> = old_tree.keys().chain(new_tree.keys()).filter(|p| matches_ls_files_pathspec(p, &specs)).collect();
    let mut pairs = Vec::new();
    for p in paths {
        let (old, new) = (old_tree.get(p).copied(), new_tree.get(p).copied());
        if old == new {
            continue;
        }
        let pair = |old, new| FollowPair { old_path: p.clone(), new_path: p.clone(), old, new, rename: None };
        match (old, new) {
            (Some(o), Some(n)) if o.1 & 0o170000 != n.1 & 0o170000 => {
                pairs.push(pair(Some(o), None));
                pairs.push(pair(None, Some(n)));
            }
            _ => pairs.push(pair(old, new)),
        }
    }

    if !pairs.iter().any(|p| p.old.is_none()) {
        return Ok((pairs, None));
    }
    let choice = match find_rename_source(&old_tree, &new_tree, path)? {
        Some(found) => {
            let source = found.old_path.clone();
            return Ok((vec![found], Some(source)));
        }
        None => pairs.swap_remove(0),
    };
    Ok((vec![choice], None))
}

/// Finds where `path` (created in `new_tree`) came from: an identical file first (one that was
/// deleted preferred, then one with the same base name), else the most similar regular file
/// scoring at least 50%. A source that no longer exists makes it a rename (`R`), one that is
/// still there a copy (`C`); renames win over copies, as in git's first matching pass.
// Used by `follow_commit_pairs`.
fn find_rename_source(old_tree: &Tree, new_tree: &Tree, path: &str) -> anyhow::Result<Option<FollowPair>> {
    let Some(&dst) = new_tree.get(path) else { return Ok(None) };
    if !is_regular(dst.1) {
        return Ok(None);
    }
    let make = |src_path: &str, src: ([u8; 20], u32), score: u64| {
        let status = if new_tree.contains_key(src_path) { 'C' } else { 'R' };
        FollowPair { old_path: src_path.to_string(), new_path: path.to_string(), old: Some(src), new: Some(dst), rename: Some((status, score as u32)) }
    };
    let basename = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let dst_base = basename(path);

    // Exact renames: same blob.
    let mut best: Option<(u32, &String)> = None;
    for (src_path, src) in old_tree {
        if src.0 != dst.0 || is_regular(src.1) != is_regular(dst.1) {
            continue;
        }
        let score = u32::from(!new_tree.contains_key(src_path)) + u32::from(basename(src_path) == dst_base);
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, src_path));
        }
    }
    if let Some((_, src_path)) = best {
        return Ok(Some(make(src_path, old_tree[src_path], MAX_SCORE)));
    }

    // Inexact: git's per-destination table of four candidates. Every source is offered (even
    // one scoring 0); a better one overwrites the first worst slot, then the slots are sorted
    // stably, so equal scores keep slot order. Renames (deleted sources) are taken first.
    let dst_data = read_object(&hex::encode(dst.0))?.1;
    let dst_hash = span_hash(&dst_data);
    let sources: Vec<(&String, &([u8; 20], u32))> = old_tree.iter().collect();
    let mut slots: [Option<(u64, bool, usize)>; 4] = [None; 4];
    for (j, (src_path, src)) in sources.iter().enumerate() {
        let score = if is_regular(src.1) {
            let src_data = read_object(&hex::encode(src.0))?.1;
            estimate_similarity(&src_data, &span_hash(&src_data), &dst_data, &dst_hash, MIN_SCORE)
        } else {
            0
        };
        let candidate = Some((score, basename(src_path) == dst_base, j));
        let mut worst = 0;
        for i in 1..slots.len() {
            if score_compare(&slots[i], &slots[worst]) == std::cmp::Ordering::Greater {
                worst = i;
            }
        }
        if score_compare(&slots[worst], &candidate) == std::cmp::Ordering::Greater {
            slots[worst] = candidate;
        }
    }
    slots.sort_by(score_compare);
    let usable: Vec<(u64, usize)> = slots.iter().flatten().take_while(|c| c.0 >= MIN_SCORE).map(|c| (c.0, c.2)).collect();
    let pick = usable.iter().find(|c| !new_tree.contains_key(sources[c.1].0)).or(usable.first());
    Ok(pick.map(|&(score, j)| make(sources[j].0, *sources[j].1, score)))
}

/// git's `score_compare` for rename candidates: unused slots last, then higher score, then a
/// matching base name first (`Greater` means "sorts after").
// Used by `find_rename_source`.
fn score_compare(a: &Option<(u64, bool, usize)>, b: &Option<(u64, bool, usize)>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(a), Some(b)) if a.0 == b.0 => b.1.cmp(&a.1),
        (Some(a), Some(b)) => b.0.cmp(&a.0),
    }
}

/// Whether a tree entry mode is a regular file (rename detection only scores those).
// Used by `find_rename_source`.
fn is_regular(mode: u32) -> bool {
    mode & 0o170000 == 0o100000
}

fn status_of(pair: &FollowPair) -> String {
    match (pair.rename, pair.old, pair.new) {
        (Some((status, score)), _, _) => format!("{}{:03}", status, score as u64 * 100 / MAX_SCORE),
        (None, None, _) => "A".to_string(),
        (None, _, None) => "D".to_string(),
        _ => "M".to_string(),
    }
}

/// Reads the content of one side of a pair (empty when missing, a stand-in line for gitlinks).
// Used by `render_follow_diff`.
fn load(entry: Option<([u8; 20], u32)>) -> anyhow::Result<Vec<u8>> {
    match entry {
        Some((hash, 0o160000)) => Ok(format!("Subproject commit {}\n", hex::encode(hash)).into_bytes()),
        Some((hash, _)) => Ok(read_object(&hex::encode(hash))?.1),
        None => Ok(Vec::new()),
    }
}

/// Renders the follow diff of one commit in the diff format `log` was asked for, as
/// `commit_diff_output` does for ordinary commits: `--name-only`, `--raw` or `--name-status`,
/// else `--stat` / `--shortstat` (at `stat_width` columns) and/or the patch. A rename or copy
/// shows as git does: `R086\told\tnew`, `old => new` in the stat, and a `similarity index` /
/// `rename from` / `rename to` header in the patch.
// Used by `log --follow`.
pub fn render_follow_diff(pairs: &[FollowPair], opts: &LogOptions, stat_width: usize) -> anyhow::Result<String> {
    let wants_diff = opts.patch || opts.stat || opts.shortstat || opts.name_only || opts.name_status || opts.raw;
    if !wants_diff || pairs.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::new();
    if opts.name_only || opts.name_status || opts.raw {
        for pair in pairs {
            let names = if pair.rename.is_some() { format!("{}\t{}", pair.old_path, pair.new_path) } else { pair.new_path.clone() };
            if opts.name_only {
                out.push_str(&format!("{}\n", pair.new_path));
            } else if opts.raw {
                let side = |e: Option<([u8; 20], u32)>| e.map(|(h, m)| (m, hex::encode(h)[..7].to_string())).unwrap_or((0, "0000000".to_string()));
                let ((om, oh), (nm, nh)) = (side(pair.old), side(pair.new));
                out.push_str(&format!(":{:06o} {:06o} {} {} {}\t{}\n", om, nm, oh, nh, status_of(pair), names));
            } else {
                out.push_str(&format!("{}\t{}\n", status_of(pair), names));
            }
        }
        return Ok(out);
    }

    let mut diffs = Vec::new();
    for pair in pairs {
        let (old_bytes, new_bytes) = (load(pair.old)?, load(pair.new)?);
        let diff = match pair.rename {
            // The same rename/copy rendering as every other diff output.
            Some((status, score)) => crate::helpers::diff_plumbing::renamed_file_diff(&pair.old_path, &pair.new_path, status, score, pair.old, old_bytes, pair.new, new_bytes)?,
            None => diff_file_contents(&pair.new_path, pair.old, pair.new, old_bytes, new_bytes)?,
        };
        diffs.push(diff);
    }
    if opts.stat || opts.shortstat {
        let stats: Vec<DiffStatEntry> = diffs.iter().map(|d| d.stat.clone()).collect();
        let block = format_diffstat(&stats, stat_width);
        if opts.stat {
            out.push_str(&block);
        } else if let Some(summary) = block.lines().last() {
            out.push_str(summary);
            out.push('\n');
        }
        if opts.patch {
            out.push('\n');
        }
    }
    if opts.patch {
        for (diff, pair) in diffs.iter().zip(pairs) {
            out.push_str(&binary_patch_as_notice(&diff.text, &pair.old_path, &pair.new_path));
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "../../tests/unit/helpers/follow.rs"]
mod tests;
