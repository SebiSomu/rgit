pub mod cli;
pub mod types;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use crate::workspace::gitignore::normalize_path;
use crate::core::commit::resolve_tree_from_source;
use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit as tree_of_commit};
use crate::core::index::{read_index, write_index, IndexEntry};
use crate::merge::{merge_file::types::MergeFileOptions, types::MergeConflictStyle};
use crate::core::refs;
use crate::workspace::restore::types::WriteoutStage;

/// Turns `--ours` / `--theirs` / `-m` / `--conflict=<style>` into the version to write out.
/// `--merge` without a style uses `merge.conflictStyle` (default `merge`).
// Used by the `restore` and `checkout -- <paths>` dispatch.
pub fn writeout_stage(ours: bool, theirs: bool, merge: bool, conflict: Option<String>) -> Result<WriteoutStage> {
    let merging = merge || conflict.is_some();
    if merging && (ours || theirs) {
        anyhow::bail!("fatal: '--merge' cannot be used with '--ours' or '--theirs'");
    }
    if merging {
        let style = match conflict {
            Some(style) => style,
            None => crate::core::config::get_config_value("merge.conflictstyle").ok().flatten().unwrap_or_else(|| "merge".into()),
        };
        return Ok(WriteoutStage::Merge(match style.as_str() {
            "merge" => MergeConflictStyle::Merge,
            "diff3" => MergeConflictStyle::Diff3,
            "zdiff3" => MergeConflictStyle::Zdiff3,
            other => anyhow::bail!("error: unknown conflict style '{}'", other),
        }));
    }
    Ok(if ours { WriteoutStage::Ours } else if theirs { WriteoutStage::Theirs } else { WriteoutStage::Index })
}

/// `rgit checkout [<tree-ish>] [--ours|--theirs|-m|--conflict=<style>] -- <paths>`
///
/// With a tree-ish, the paths are copied from it into both the index and the working tree
/// (this also resolves conflicts). Without one, the working tree is restored from the index;
/// an unmerged path needs `--ours` / `--theirs` (that stage, an error if the side doesn't have
/// the path) or `--merge` (the conflict recreated), and is otherwise an error.
pub fn checkout_paths(target: Option<String>, stage: WriteoutStage, paths: Vec<PathBuf>) -> Result<()> {
    match target {
        Some(_) if stage != WriteoutStage::Index => {
            eprintln!("fatal: '--merge', '--ours', or '--theirs' cannot be used when checking out of a tree");
            crate::ui::exit(128);
        }
        Some(tree) => {
            let rel: Vec<String> = paths.iter().map(|p| normalize_path(p)).collect();
            restore_from_tree(&rel, &resolve_tree_from_source(&tree)?, true, true, true)
        }
        None => {
            let rel: Vec<String> = paths.iter().map(|p| normalize_path(p)).collect();
            if !write_from_index(&rel, stage, true)? {
                crate::ui::exit(1);
            }
            Ok(())
        }
    }
}

/// Writes each path's index version to the working tree, handling unmerged paths as `stage`
/// says. In `overlay` mode (`checkout`) a side that lacks the path is an error; otherwise
/// (`restore`) the file is removed. Like git, every path is checked first: if any fails, the
/// errors are printed, nothing is written, and false is returned.
fn write_from_index(paths: &[String], stage: WriteoutStage, overlay: bool) -> Result<bool> {
    let entries = read_index().unwrap_or_default();
    let mut ok = true;
    let mut writes: Vec<(&String, Option<(Vec<u8>, u32)>)> = Vec::new();
    for path in paths {
        let stages: Vec<&IndexEntry> = entries.iter().filter(|e| &e.path == path).collect();
        if stages.is_empty() {
            eprintln!("error: pathspec '{}' did not match any file(s) known to rgit", path);
            ok = false;
            continue;
        }
        let at = |n: u8| stages.iter().find(|e| e.stage == n).copied();
        let content: Option<(Vec<u8>, u32)> = if let Some(entry) = at(0) {
            Some((read_object(&hex::encode(entry.hash))?.1, entry.mode))
        } else {
            match stage {
                WriteoutStage::Index => {
                    eprintln!("error: path '{}' is unmerged", path);
                    ok = false;
                    continue;
                }
                WriteoutStage::Ours | WriteoutStage::Theirs => {
                    let (n, which) = if stage == WriteoutStage::Ours { (2, "our") } else { (3, "their") };
                    match at(n) {
                        Some(entry) => Some((read_object(&hex::encode(entry.hash))?.1, entry.mode)),
                        None if overlay => {
                            eprintln!("error: path '{}' does not have {} version", path, which);
                            ok = false;
                            continue;
                        }
                        None => None,
                    }
                }
                WriteoutStage::Merge(style) => {
                    let (Some(ours), Some(theirs)) = (at(2), at(3)) else {
                        eprintln!("error: path '{}' does not have all necessary versions", path);
                        ok = false;
                        continue;
                    };
                    let bytes = |e: Option<&IndexEntry>| -> Result<Vec<u8>> {
                        Ok(match e {
                            Some(e) => read_object(&hex::encode(e.hash))?.1,
                            None => Vec::new(),
                        })
                    };
                    let (base, our_bytes, their_bytes) = (bytes(at(1))?, bytes(Some(ours))?, bytes(Some(theirs))?);
                    let binary = |b: &[u8]| b[..b.len().min(8000)].contains(&0);
                    if binary(&base) || binary(&our_bytes) || binary(&their_bytes) {
                        // Like git's binary merge driver: our version, with a warning.
                        eprintln!("warning: Cannot merge binary files: {} (ours vs. theirs)", path);
                        Some((our_bytes, ours.mode))
                    } else {
                        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
                        let opts = MergeFileOptions { ours_label: "ours".into(), base_label: "base".into(), theirs_label: "theirs".into(), style, ..Default::default() };
                        let merged = crate::merge::merge_tree::engine::merge_file_lines_with(&text(&base), &text(&our_bytes), &text(&their_bytes), &opts);
                        Some((merged.content, ours.mode))
                    }
                }
            }
        };
        writes.push((path, content));
    }
    if !ok {
        return Ok(false);
    }
    for (path, content) in writes {
        match content {
            Some((bytes, mode)) => write_worktree_file(path, &bytes, mode)?,
            None => {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(true)
}

fn write_worktree_file(path: &str, bytes: &[u8], mode: u32) -> Result<()> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, crate::core::eol::smudge(path, bytes)).with_context(|| format!("failed to restore '{}'", path))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(if mode & 0o111 != 0 { 0o755 } else { 0o644 }))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

pub fn restore(files: Vec<PathBuf>, staged: bool, worktree: bool, source: Option<String>, stage: WriteoutStage) -> Result<()> {
    if files.is_empty() {
        anyhow::bail!("fatal: you must specify path(s) to restore");
    }
    if staged && matches!(stage, WriteoutStage::Ours | WriteoutStage::Theirs) {
        eprintln!("fatal: '--ours' or '--theirs' cannot be used with --staged");
        crate::ui::exit(128);
    }
    if staged && matches!(stage, WriteoutStage::Merge(_)) {
        eprintln!("fatal: '--merge' or '--conflict' cannot be used with --staged");
        crate::ui::exit(128);
    }
    if source.is_some() && stage != WriteoutStage::Index {
        eprintln!("fatal: '--merge', '--ours', or '--theirs' cannot be used when checking out of a tree");
        crate::ui::exit(128);
    }
    // Plain `restore <paths>`: the working tree from the index, unmerged paths included.
    if !staged && source.is_none() {
        let rel: Vec<String> = files.iter().map(|p| normalize_path(p)).collect();
        if !write_from_index(&rel, stage, false)? {
            crate::ui::exit(1);
        }
        return Ok(());
    }

    let tree: BTreeMap<String, ([u8; 20], u32)> = match &source {
        Some(src) => resolve_tree_from_source(src)?,
        None => match refs::resolve_head_commit()? {
            Some(commit_hash) => {
                let mut tree_map = BTreeMap::new();
                flatten_tree(&tree_of_commit(&commit_hash)?, "", &mut tree_map)?;
                tree_map
            }
            None => BTreeMap::new(),
        },
    };
    let rel: Vec<String> = files.iter().map(|p| normalize_path(p)).collect();
    restore_from_tree(&rel, &tree, worktree || !staged, staged, false)
}

/// Copies paths from a tree into the working tree and/or the index, like git's
/// `checkout_paths` with a source tree: every path is checked first and nothing changes if
/// any fails (a path neither the tree nor the index has, or an unmerged path the tree doesn't
/// have, which the tree can't resolve). A path the tree has replaces all of its index stages.
/// A path only the index has is removed (`restore`, no-overlay mode), or is a pathspec error
/// (`overlay`, `checkout <tree> -- <path>`).
fn restore_from_tree(paths: &[String], tree: &BTreeMap<String, ([u8; 20], u32)>, do_worktree: bool, do_staged: bool, overlay: bool) -> Result<()> {
    let mut index_entries = read_index().unwrap_or_default();
    let mut errors: Vec<String> = Vec::new();
    for path in paths {
        let in_index = index_entries.iter().any(|e| &e.path == path);
        if tree.contains_key(path) {
            continue;
        }
        if !in_index || overlay {
            errors.push(format!("error: pathspec '{}' did not match any file(s) known to rgit", path));
        } else if index_entries.iter().any(|e| &e.path == path && e.stage != 0) {
            errors.push(format!("error: path '{}' is unmerged", path));
        }
    }
    if !errors.is_empty() {
        for error in errors {
            eprintln!("{}", error);
        }
        crate::ui::exit(1);
    }

    for path in paths {
        match tree.get(path) {
            Some(&(hash, mode)) => {
                let (_, content) = read_object(&hex::encode(hash))?;
                if do_worktree {
                    write_worktree_file(path, &content, mode)?;
                }
                if do_staged {
                    index_entries.retain(|e| &e.path != path);
                    let mut entry = if do_worktree {
                        crate::core::index::build_entry(path, hash, &fs::metadata(path)?)
                    } else {
                        IndexEntry::unmerged(path, hash, mode, 0)
                    };
                    entry.mode = mode;
                    entry.size = content.len() as u32;
                    index_entries.push(entry);
                }
            }
            None => {
                if do_worktree {
                    let _ = fs::remove_file(path);
                }
                if do_staged {
                    index_entries.retain(|e| &e.path != path);
                }
            }
        }
    }
    if do_staged {
        write_index(&mut index_entries)?;
    }
    Ok(())
}

