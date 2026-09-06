use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::helpers::{
    build_index_entries_for_tree, commit_subject_line, flatten_tree, hard_reset_working_tree,
    normalize_path, read_object, resolve_commit_from_source, resolve_tree_from_source,
    tree_hash_of_commit,
};
use crate::index::{read_index, write_index, IndexEntry};
use crate::objects::ResetMode;
use crate::refs;

fn reset_paths(source: Option<&str>, paths: &[PathBuf]) -> anyhow::Result<()> {
    let source_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(src) = source {
        resolve_tree_from_source(src)?
    } else {
        match refs::resolve_head_commit()? {
            Some(commit_hash) => {
                let tree_hash = tree_hash_of_commit(&commit_hash)?;
                let mut map = BTreeMap::new();
                flatten_tree(&tree_hash, "", &mut map)?;
                map
            }
            // No commits yet: HEAD is effectively an empty tree, so pathspecs simply unstage.
            None => BTreeMap::new(),
        }
    };

    let mut index_entries = read_index().unwrap_or_default();

    for path_buf in paths {
        let rel_path = normalize_path(path_buf);

        let mut matches: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for entry in &index_entries {
            if entry.path == rel_path || entry.path.starts_with(&format!("{}/", rel_path)) {
                matches.insert(entry.path.clone());
            }
        }
        for src_path in source_tree.keys() {
            if src_path == &rel_path || src_path.starts_with(&format!("{}/", rel_path)) {
                matches.insert(src_path.clone());
            }
        }

        if matches.is_empty() {
            anyhow::bail!(
                "fatal: pathspec '{}' did not match any file(s) known to rgit",
                rel_path
            );
        }

        for m in matches {
            match source_tree.get(&m) {
                Some(&(hash, mode)) => {
                    if let Some(entry) = index_entries.iter_mut().find(|e| e.path == m) {
                        entry.hash = hash;
                        entry.mode = mode;
                    } else {
                        let (_, content) = read_object(&hex::encode(hash))?;
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
                            hash,
                            path: m.clone(),
                        });
                    }
                }
                None => {
                    index_entries.retain(|e| e.path != m);
                }
            }
        }
    }

    write_index(&mut index_entries)?;
    Ok(())
}

pub fn reset(commit: Option<String>, soft: bool, _mixed: bool, hard: bool, paths: Vec<PathBuf>) -> anyhow::Result<()> {
    if !paths.is_empty() {
        if soft {
            anyhow::bail!("fatal: Cannot do soft reset with paths.");
        }
        if hard {
            anyhow::bail!("fatal: Cannot do hard reset with paths.");
        }
        return reset_paths(commit.as_deref(), &paths);
    }

    let mode = if hard {
        ResetMode::Hard
    } else if soft {
        ResetMode::Soft
    } else {
        ResetMode::Mixed
    };

    let source = commit.as_deref().unwrap_or("HEAD");
    let target_commit = resolve_commit_from_source(source)?;

    let head_state = refs::resolve_head()?;
    let old_head_commit = refs::resolve_head_commit()?;

    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            let ref_path = format!("refs/heads/{}", branch_name);
            refs::write_ref(&ref_path, &target_commit)?;
        }
        refs::HeadState::Detached(_) => {
            refs::set_head_detached(&target_commit)?;
        }
    }

    if Path::new(".git/MERGE_HEAD").exists() {
        let _ = fs::remove_file(".git/MERGE_HEAD");
    }
    if Path::new(".git/MERGE_MSG").exists() {
        let _ = fs::remove_file(".git/MERGE_MSG");
    }

    if let ResetMode::Soft = mode {
        return Ok(());
    }

    let target_tree_hash = tree_hash_of_commit(&target_commit)?;
    let mut target_tree = BTreeMap::new();
    flatten_tree(&target_tree_hash, "", &mut target_tree)?;

    let mut old_head_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
    if let Some(ref old_commit) = old_head_commit {
        let old_tree_hash = tree_hash_of_commit(old_commit)?;
        flatten_tree(&old_tree_hash, "", &mut old_head_tree)?;
    }

    let old_index_entries = read_index().unwrap_or_default();
    let old_index_map: BTreeMap<String, [u8; 20]> = old_index_entries
        .iter()
        .map(|e| (e.path.clone(), e.hash))
        .collect();

    if let ResetMode::Hard = mode {
        let mut tracked_paths: std::collections::BTreeSet<String> = old_index_map.keys().cloned().collect();
        tracked_paths.extend(old_head_tree.keys().cloned());
        hard_reset_working_tree(&target_tree, &tracked_paths)?;
    }

    let read_disk_metadata = matches!(mode, ResetMode::Hard);
    let mut new_entries = build_index_entries_for_tree(&target_tree, read_disk_metadata)?;
    write_index(&mut new_entries)?;

    match mode {
        ResetMode::Hard => {
            let subject = commit_subject_line(&target_commit)?;
            println!("HEAD is now at {} {}", &target_commit[..7], subject);
        }
        ResetMode::Mixed => {
            print_unstaged_after_reset(&old_index_map, &target_tree);
        }
        ResetMode::Soft => unreachable!("soft reset returns earlier"),
    }

    Ok(())
}

fn print_unstaged_after_reset(
    old_index_map: &BTreeMap<String, [u8; 20]>,
    target_tree: &BTreeMap<String, ([u8; 20], u32)>,
) {
    let mut changes: Vec<(String, char)> = Vec::new();

    for (path, old_hash) in old_index_map {
        match target_tree.get(path) {
            None => changes.push((path.clone(), 'D')),
            Some((new_hash, _)) if new_hash != old_hash => changes.push((path.clone(), 'M')),
            _ => {}
        }
    }
    for path in target_tree.keys() {
        if !old_index_map.contains_key(path) {
            changes.push((path.clone(), 'A'));
        }
    }

    if changes.is_empty() {
        return;
    }

    changes.sort_by(|a, b| a.0.cmp(&b.0));

    println!("Unstaged changes after reset:");
    for (path, status) in changes {
        println!("{}\t{}", status, path);
    }
}