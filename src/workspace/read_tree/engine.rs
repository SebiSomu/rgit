use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::workspace::index_plumbing::{bare_index_entry, worktree_state};
use crate::core::object_store::flatten_tree;
use crate::core::rev_parse::{peel_to_type, resolve_rev};
use crate::core::index::IndexEntry;
use crate::objects::{IndexFileState, ReadTreeOptions, ReadTreePlan};

/// Usage text of `read-tree`, printed for `-h` and usage errors.
pub const READ_TREE_USAGE: &str = "usage: rgit read-tree [(-m [--trivial] [--aggressive] | --reset | --prefix=<prefix>)\n                      [-u | -i]] [-n] (--empty | <tree-ish1> [<tree-ish2> [<tree-ish3>]])\n\n    -m                    perform a merge in addition to a read\n    --reset               same as -m, but discard local changes\n    --prefix <subdirectory>/\n                          read the tree into the index under <subdirectory>/\n    -u                    update working tree with merge result\n    -i                    don't check the working tree after merging\n    -n, --dry-run         don't update the index or the work tree\n    --trivial             3-way merge if no file level merging required\n    --aggressive          3-way merge in presence of adds and removes\n    --empty               only empty the index\n\n";

/// Parses the arguments of `read-tree`. `Err` is `(text, exit status)`.
// Used by `read-tree`.
pub fn parse_read_tree_args(args: &[String]) -> Result<ReadTreeOptions, (String, i32)> {
    let mut opts = ReadTreeOptions::default();
    let usage = |message: String| (format!("{}\n{}", message, READ_TREE_USAGE), 129);
    let mut i = 0;
    let mut options_done = false;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if options_done || !arg.starts_with('-') || arg == "-" {
            opts.trees.push(arg.to_string());
            continue;
        }
        match arg {
            "--" => options_done = true,
            "-h" => return Err((READ_TREE_USAGE.to_string(), 129)),
            "-m" => opts.merge = true,
            "--reset" => opts.reset = true,
            "-u" => opts.update = true,
            "-i" => opts.index_only = true,
            "-n" | "--dry-run" => opts.dry_run = true,
            "--trivial" => opts.trivial = true,
            "--aggressive" => opts.aggressive = true,
            "--empty" => opts.empty = true,
            "-v" | "--verbose" | "--no-sparse-checkout" | "--sparse-checkout" | "--recurse-submodules" | "--no-recurse-submodules" | "-q" | "--quiet" => {}
            "--prefix" => {
                let value = args.get(i).cloned().ok_or_else(|| ("error: option `prefix' requires a value\n".to_string(), 129))?;
                i += 1;
                opts.prefix = Some(value);
            }
            other if other.starts_with("--prefix=") => opts.prefix = Some(other["--prefix=".len()..].to_string()),
            other if other.starts_with("--exclude-per-directory=") => {}
            other if other.starts_with("--index-output") => return Err(("fatal: --index-output is not supported by rgit".to_string() + "\n", 128)),
            other if other.starts_with("--") => return Err(usage(format!("error: unknown option `{}'", &other[2..]))),
            other => return Err(usage(format!("error: unknown switch `{}'", other.chars().nth(1).unwrap_or('-')))),
        }
    }
    let fatal = |message: &str| (format!("fatal: {}\n", message), 128);
    if let Some(prefix) = &opts.prefix {
        if prefix.starts_with('/') {
            return Err(fatal("Invalid prefix, prefix cannot start with '/'"));
        }
    }
    if [opts.merge, opts.reset, opts.prefix.is_some()].iter().filter(|on| **on).count() > 1 {
        return Err(fatal("Which one? -m, --reset, or --prefix?"));
    }
    if opts.empty && (!opts.trees.is_empty() || opts.merge || opts.reset || opts.prefix.is_some()) {
        return Err(fatal("passing trees as arguments contradicts --empty"));
    }
    if opts.update && !(opts.merge || opts.reset || opts.prefix.is_some()) {
        return Err(fatal("-u is meaningless without -m, --reset, or --prefix"));
    }
    if opts.index_only && opts.update {
        return Err(fatal("-u and -i at the same time makes no sense"));
    }
    if (opts.merge || opts.reset) && opts.trees.is_empty() {
        return Err(fatal("you must specify at least one tree to merge"));
    }
    Ok(opts)
}

/// Resolves a tree-ish to its flattened file list. Errors are git's `fatal:` lines.
// Used by `plan_read_tree`.
fn load_tree(rev: &str) -> Result<BTreeMap<String, ([u8; 20], u32)>, String> {
    let hash = resolve_rev(rev).map_err(|_| format!("fatal: Not a valid object name {}", rev))?;
    let tree = peel_to_type(hash.clone(), "tree").map_err(|_| format!("fatal: failed to unpack tree object {}", hash))?;
    let mut map = BTreeMap::new();
    flatten_tree(&tree, "", &mut map).map_err(|_| format!("fatal: failed to unpack tree object {}", hash))?;
    Ok(map)
}

/// git's `verify_uptodate`: the file of `old` must not have local changes (unless `-i` or
/// `--reset`). Records the error and returns false otherwise.
// Used by `merged_entry`, `deleted_entry` and `plan_read_tree`.
fn verify_uptodate(opts: &ReadTreeOptions, plan: &mut ReadTreePlan, old: &IndexEntry) -> bool {
    if opts.index_only || opts.reset {
        return true;
    }
    if let IndexFileState::Modified(_) = worktree_state(old) {
        plan.errors.push(format!("error: Entry '{}' not uptodate. Cannot merge.", old.path));
        return false;
    }
    true
}

/// git's `verify_absent`: with `-u`, an untracked file must not be in the way of a new entry
/// (unless `--reset`). Records the error and returns false otherwise.
// Used by `merged_entry`.
fn verify_absent(opts: &ReadTreeOptions, plan: &mut ReadTreePlan, path: &str) -> bool {
    if !opts.update || opts.reset {
        return true;
    }
    if Path::new(path).is_file() {
        plan.errors.push(format!("error: Untracked working tree file '{}' would be overwritten by merge.", path));
        return false;
    }
    true
}

/// git's `merged_entry`: the path gets the tree's version. An index entry that already has
/// it is kept with its stat data; otherwise the old file must be unmodified (or no untracked
/// file may be in the way) and the path is queued for writing.
// Used by `plan_read_tree`.
fn merged_entry(opts: &ReadTreeOptions, plan: &mut ReadTreePlan, path: &str, new: ([u8; 20], u32), old: Option<&IndexEntry>) {
    match old {
        Some(old) if (old.hash, old.mode) == new => {
            // Same content: keep the entry with its stat data. `--reset -u` still restores the file.
            if opts.reset && opts.update && worktree_state(old) != IndexFileState::Clean {
                plan.write.push(path.to_string());
            }
            plan.entries.push(old.clone());
        }
        Some(old) => {
            if verify_uptodate(opts, plan, old) {
                plan.entries.push(bare_index_entry(path, new.0, new.1));
                plan.write.push(path.to_string());
            }
        }
        None => {
            if verify_absent(opts, plan, path) {
                plan.entries.push(bare_index_entry(path, new.0, new.1));
                plan.write.push(path.to_string());
            }
        }
    }
}

/// git's `deleted_entry`: the path leaves the index; its file must be unmodified and is
/// queued for removal.
// Used by `plan_read_tree`.
fn deleted_entry(opts: &ReadTreeOptions, plan: &mut ReadTreePlan, old: Option<&IndexEntry>) {
    if let Some(old) = old {
        if verify_uptodate(opts, plan, old) {
            plan.remove.push(old.path.clone());
        }
    }
}

/// git's `reject_merge`: the index entry clashes with what the merge would put there.
// Used by `plan_read_tree`.
fn reject_merge(plan: &mut ReadTreePlan, path: &str) {
    plan.errors.push(format!("error: Entry '{}' would be overwritten by merge. Cannot merge.", path));
}

/// Works out what `read-tree` does, without touching anything: reads the trees and applies
/// git's rules — plain read (later trees override earlier ones), `--prefix`, the one-way
/// merge (`-m <tree>`, `--reset`), the two-way merge (`-m <old> <new>`: local index changes
/// are carried over when they do not clash) and the three-way merge (`-m <base>... <ours>
/// <theirs>`: trivial cases only, plus deletions and identical additions with
/// `--aggressive`). `Err` is a `fatal:` line; problems with single entries are in the plan.
// Used by `read-tree`.
pub fn plan_read_tree(opts: &ReadTreeOptions, index: &[IndexEntry]) -> Result<ReadTreePlan, String> {
    let mut trees = Vec::new();
    for rev in &opts.trees {
        trees.push(load_tree(rev)?);
    }
    let by_path: BTreeMap<&str, &IndexEntry> = index.iter().map(|e| (e.path.as_str(), e)).collect();
    let mut plan = ReadTreePlan::default();

    if let Some(prefix) = &opts.prefix {
        let prefix = if prefix.is_empty() || prefix.ends_with('/') { prefix.clone() } else { format!("{}/", prefix) };
        plan.entries = index.to_vec();
        for tree in &trees {
            for (path, (hash, mode)) in tree {
                let full = format!("{}{}", prefix, path);
                let clash = index.iter().find(|e| e.path == full || e.path.starts_with(&format!("{}/", full)) || full.starts_with(&format!("{}/", e.path)));
                if let Some(existing) = clash {
                    return Err(format!("error: Entry '{}' overlaps with '{}'.  Cannot bind.", full, existing.path));
                }
                plan.entries.retain(|e| e.path != full);
                plan.entries.push(bare_index_entry(&full, *hash, *mode));
                plan.write.push(full);
            }
        }
        return Ok(plan);
    }

    if !(opts.merge || opts.reset) {
        // Plain read: the index becomes the trees, later ones overriding earlier ones.
        let mut combined: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
        for tree in trees {
            combined.extend(tree);
        }
        plan.entries = combined.iter().map(|(path, (hash, mode))| bare_index_entry(path, *hash, *mode)).collect();
        return Ok(plan);
    }

    let mut paths: BTreeSet<&str> = by_path.keys().copied().collect();
    for tree in &trees {
        paths.extend(tree.keys().map(String::as_str));
    }
    let same = |a: Option<&IndexEntry>, b: Option<&([u8; 20], u32)>| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a.hash, a.mode) == *b,
        _ => false,
    };

    for path in paths {
        let current = by_path.get(path).copied();
        match trees.len() {
            1 => match trees[0].get(path) {
                Some(new) => merged_entry(opts, &mut plan, path, *new, current),
                None => deleted_entry(opts, &mut plan, current),
            },
            2 => {
                let (old, new) = (trees[0].get(path), trees[1].get(path));
                match current {
                    Some(cur) => {
                        let keeps = (old.is_none() && new.is_none())
                            || (old.is_none() && same(Some(cur), new))
                            || (old.is_some() && new.is_some() && old == new)
                            || (old.is_some() && new.is_some() && old != new && same(Some(cur), new));
                        if keeps {
                            plan.entries.push(cur.clone());
                        } else if old.is_some() && new.is_none() && same(Some(cur), old) {
                            deleted_entry(opts, &mut plan, Some(cur));
                        } else if old.is_some() && new.is_some() && same(Some(cur), old) && !same(Some(cur), new) {
                            merged_entry(opts, &mut plan, path, *new.unwrap_or(&([0; 20], 0)), Some(cur));
                        } else {
                            reject_merge(&mut plan, path);
                        }
                    }
                    None => match (old, new) {
                        // The deletion of the path was staged: fine if the new tree did not change it.
                        (Some(old), Some(new)) => {
                            if old != new {
                                reject_merge(&mut plan, path);
                            }
                        }
                        (None, Some(new)) => merged_entry(opts, &mut plan, path, *new, None),
                        _ => {}
                    },
                }
            }
            _ => {
                let remote = trees[trees.len() - 1].get(path);
                let head = trees[trees.len() - 2].get(path);
                let ancestors: Vec<Option<&([u8; 20], u32)>> = trees[..trees.len() - 2].iter().map(|t| t.get(path)).collect();
                let any_anc_missing = ancestors.iter().any(|a| a.is_none());
                let no_anc_exists = ancestors.iter().all(|a| a.is_none());
                let (mut head_match, mut remote_match) = (false, false);
                if remote != head {
                    head_match = ancestors.iter().any(|a| *a == head);
                    remote_match = ancestors.iter().any(|a| *a == remote);
                }

                if let Some(remote) = remote.filter(|_| head_match && !remote_match) {
                    if current.is_some() && !same(current, Some(remote)) && !same(current, head) {
                        reject_merge(&mut plan, path);
                    } else {
                        merged_entry(opts, &mut plan, path, *remote, current);
                    }
                    continue;
                }
                if current.is_some() && !same(current, head) {
                    reject_merge(&mut plan, path);
                    continue;
                }
                if let Some(head_entry) = head {
                    if head == remote || (remote_match && !head_match) {
                        merged_entry(opts, &mut plan, path, *head_entry, current);
                        continue;
                    }
                }
                if head.is_none() && remote.is_none() && any_anc_missing {
                    continue;
                }
                if opts.aggressive {
                    let (head_deleted, remote_deleted) = (head.is_none(), remote.is_none());
                    if (head_deleted && remote_deleted) || (head_deleted && remote_match) || (remote_deleted && head_match) {
                        deleted_entry(opts, &mut plan, current);
                        continue;
                    }
                    if let Some(head_entry) = head.filter(|_| no_anc_exists && head == remote) {
                        merged_entry(opts, &mut plan, path, *head_entry, current);
                        continue;
                    }
                }
                // A real file-level merge: git would leave stages 1-3 here.
                if let Some(cur) = current {
                    verify_uptodate(opts, &mut plan, cur);
                }
                plan.unmerged.push(path.to_string());
            }
        }
    }
    Ok(plan)
}
