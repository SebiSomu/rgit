use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use crate::helpers::{flatten_tree, normalize_path, read_object, resolve_tree_from_source, tree_hash_of_commit};
use crate::index::{read_index, write_index, IndexEntry};
use crate::refs;

pub fn restore(files: Vec<PathBuf>, staged: bool, worktree: bool, source: Option<String>) -> Result<()> {
    if files.is_empty() {
        anyhow::bail!("fatal: you must specify path(s) to restore");
    }

    let do_worktree = worktree || (!staged && !worktree);
    let do_staged = staged;
    let mut index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, ([u8; 20], u32)> = index_entries
        .iter()
        .map(|e| (e.path.clone(), (e.hash, e.mode)))
        .collect();

    let explicit_source_tree: Option<BTreeMap<String, ([u8; 20], u32)>> = if let Some(ref src) = source {
        Some(resolve_tree_from_source(src)?)
    } else {
        None
    };

    let head_tree_for_staged: Option<BTreeMap<String, ([u8; 20], u32)>> = if do_staged && source.is_none() {
        match refs::resolve_head_commit()? {
            Some(commit_hash) => {
                let tree_hash = tree_hash_of_commit(&commit_hash)?;
                let mut tree_map = BTreeMap::new();
                flatten_tree(&tree_hash, "", &mut tree_map)?;
                Some(tree_map)
            }
            None => None,
        }
    } else {
        None
    };

    let mut index_dirty = false;

    for file_path in &files {
        let rel_path = normalize_path(file_path);

        if do_worktree {
            let (blob_hash, _mode) = if let Some(ref src_tree) = explicit_source_tree {
                src_tree.get(&rel_path).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "error: pathspec '{}' did not match any file(s) known to rgit",
                        rel_path
                    )
                })?
            } else if do_staged {
                let src_tree = head_tree_for_staged.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("error: could not restore '{}': HEAD has no commits yet", rel_path)
                })?;
                src_tree.get(&rel_path).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "error: pathspec '{}' did not match any file(s) known to rgit",
                        rel_path
                    )
                })?
            } else {
                index_map.get(&rel_path).copied().ok_or_else(|| {
                    anyhow::anyhow!(
                        "error: pathspec '{}' did not match any file(s) known to rgit",
                        rel_path
                    )
                })?
            };

            let blob_hash_hex = hex::encode(blob_hash);
            let (obj_type, content) = read_object(&blob_hash_hex)?;
            if obj_type != "blob" {
                anyhow::bail!(
                    "internal error: expected blob for '{}', got {}",
                    rel_path,
                    obj_type
                );
            }

            if let Some(parent) = Path::new(&rel_path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(&rel_path, &content)
                .with_context(|| format!("failed to restore '{}'", rel_path))?;
        }

        if do_staged {
            let source_tree = explicit_source_tree.as_ref().or(head_tree_for_staged.as_ref());

            if let Some(src_tree) = source_tree {
                if let Some(&(blob_hash, mode)) = src_tree.get(&rel_path) {
                    let existing = index_entries.iter_mut().find(|e| e.path == rel_path);
                    if let Some(entry) = existing {
                        entry.hash = blob_hash;
                        entry.mode = mode;
                    } else {
                        let blob_hash_hex = hex::encode(blob_hash);
                        let (_, content) = read_object(&blob_hash_hex)?;
                        index_entries.push(IndexEntry {
                            ctime_secs: 0,
                            ctime_nsecs: 0,
                            mtime_secs: 0,
                            mtime_nsecs: 0,
                            dev: 0,
                            ino: 0,
                            mode,
                            uid: 0,
                            gid: 0,
                            size: content.len() as u32,
                            hash: blob_hash,
                            path: rel_path.clone(),
                        });
                    }
                    index_dirty = true;
                } else {
                    let before = index_entries.len();
                    index_entries.retain(|e| e.path != rel_path);
                    if index_entries.len() == before {
                        anyhow::bail!(
                            "error: pathspec '{}' did not match any file(s) known to rgit",
                            rel_path
                        );
                    }
                    index_dirty = true;
                }
            } else {
                let before = index_entries.len();
                index_entries.retain(|e| e.path != rel_path);
                if index_entries.len() == before {
                    anyhow::bail!(
                        "error: pathspec '{}' did not match any file(s) known to rgit",
                        rel_path
                    );
                }
                index_dirty = true;
            }
        }
    }

    if index_dirty {
        write_index(&mut index_entries)?;
    }

    Ok(())
}