use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use crate::helpers::{collect_files, commit_parents, flatten_tree, format_diff_output, normalize_path, read_object, resolve_commit_from_source, resolve_tree_from_source, tree_hash_of_commit};
use crate::index::read_index;
use crate::refs;

pub fn diff(staged: bool, commit_a: Option<String>, commit_b: Option<String>, paths: Vec<PathBuf>) -> anyhow::Result<()> {
    let path_filters: Vec<String> = paths.iter().map(|p| normalize_path(p)).collect();
    let should_process = |path: &str| -> bool {
        if path_filters.is_empty() {
            return true;
        }
        path_filters.iter().any(|f| path == f || path.starts_with(&format!("{}/", f)))
    };

    let mut diff_entries: BTreeMap<String, (Vec<u8>, Vec<u8>, String, String)> = BTreeMap::new();

    if let (Some(ca), Some(cb)) = (&commit_a, &commit_b) {
        let tree_a = resolve_tree_from_source(ca)?;
        let tree_b = resolve_tree_from_source(cb)?;

        let mut all_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in tree_a.keys() { all_paths.insert(p.clone()); }
        for p in tree_b.keys() { all_paths.insert(p.clone()); }

        for path in all_paths {
            if !should_process(&path) { continue; }

            let old_bytes = if let Some((hash, _)) = tree_a.get(&path) {
                read_object(&hex::encode(hash))?.1
            } else {
                Vec::new()
            };

            let new_bytes = if let Some((hash, _)) = tree_b.get(&path) {
                read_object(&hex::encode(hash))?.1
            } else {
                Vec::new()
            };

            let old_label = format!("a/{}", path);
            let new_label = format!("b/{}", path);
            diff_entries.insert(path, (old_bytes, new_bytes, old_label, new_label));
        }
    } else if let Some(ca) = &commit_a {
        let tree_a = resolve_tree_from_source(ca)?;

        let mut working_files = Vec::new();
        collect_files(Path::new("."), &mut working_files)?;

        let mut all_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in tree_a.keys() { all_paths.insert(p.clone()); }
        for f in &working_files { all_paths.insert(normalize_path(f)); }

        for path in all_paths {
            if !should_process(&path) { continue; }

            let old_bytes = if let Some((hash, _)) = tree_a.get(&path) {
                read_object(&hex::encode(hash))?.1
            } else {
                Vec::new()
            };

            let new_bytes = if Path::new(&path).exists() && Path::new(&path).is_file() {
                fs::read(&path).unwrap_or_default()
            } else {
                Vec::new()
            };

            let old_label = format!("a/{}", path);
            let new_label = format!("b/{}", path);
            diff_entries.insert(path, (old_bytes, new_bytes, old_label, new_label));
        }
    } else if staged {
        let head_tree = if let Some(commit_hash) = refs::resolve_head_commit()? {
            let tree_hash = tree_hash_of_commit(&commit_hash)?;
            let mut map = BTreeMap::new();
            flatten_tree(&tree_hash, "", &mut map)?;
            map
        } else {
            BTreeMap::new()
        };

        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

        let mut all_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in head_tree.keys() { all_paths.insert(p.clone()); }
        for p in index_map.keys() { all_paths.insert(p.clone()); }

        for path in all_paths {
            if !should_process(&path) { continue; }

            let old_bytes = if let Some((hash, _)) = head_tree.get(&path) {
                read_object(&hex::encode(hash))?.1
            } else {
                Vec::new()
            };

            let new_bytes = if let Some(hash) = index_map.get(&path) {
                read_object(&hex::encode(hash))?.1
            } else {
                Vec::new()
            };

            let old_label = format!("a/{}", path);
            let new_label = format!("b/{}", path);
            diff_entries.insert(path, (old_bytes, new_bytes, old_label, new_label));
        }
    } else {
        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

        let mut working_files = Vec::new();
        collect_files(Path::new("."), &mut working_files)?;

        let mut all_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for p in index_map.keys() { all_paths.insert(p.clone()); }
        for f in &working_files {
            let rel = normalize_path(f);
            if index_map.contains_key(&rel) {
                all_paths.insert(rel);
            }
        }

        for path in all_paths {
            if !should_process(&path) { continue; }

            let old_bytes = if let Some(hash) = index_map.get(&path) {
                read_object(&hex::encode(hash))?.1
            } else {
                Vec::new()
            };

            let new_bytes = if Path::new(&path).exists() && Path::new(&path).is_file() {
                fs::read(&path).unwrap_or_default()
            } else {
                Vec::new()
            };

            let old_label = format!("a/{}", path);
            let new_label = format!("b/{}", path);
            diff_entries.insert(path, (old_bytes, new_bytes, old_label, new_label));
        }
    }

    for (path, (old_bytes, new_bytes, old_label, new_label)) in diff_entries {
        if old_bytes == new_bytes {
            continue;
        }

        let old_str = String::from_utf8_lossy(&old_bytes);
        let new_str = String::from_utf8_lossy(&new_bytes);
        let old_lines: Vec<&str> = if old_bytes.is_empty() { Vec::new() } else { old_str.lines().collect() };
        let new_lines: Vec<&str> = if new_bytes.is_empty() { Vec::new() } else { new_str.lines().collect() };
        let formatted = format_diff_output(&path, &old_lines, &new_lines, &old_label, &new_label);
        print!("{}", formatted);
    }

    Ok(())
}

pub fn show(commit: Option<String>) -> anyhow::Result<()> {
    let source = commit.as_deref().unwrap_or("HEAD");
    let commit_hash = resolve_commit_from_source(source)?;

    let (object_type, content) = read_object(&commit_hash)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' does not point to a commit object", source);
    }
    let text = String::from_utf8_lossy(&content).to_string();

    let mut author_line: Option<String> = None;
    let mut message_lines: Vec<String> = Vec::new();
    let mut in_message = false;
    for line in text.lines() {
        if in_message {
            message_lines.push(line.to_string());
        } else if line.is_empty() {
            in_message = true;
        } else if let Some(a) = line.strip_prefix("author ") {
            author_line = Some(a.to_string());
        }
    }

    println!("commit {}", commit_hash);
    if let Some(author) = &author_line {
        println!("Author: {}", author);
    }
    println!();
    for line in &message_lines {
        println!("    {}", line);
    }
    println!();

    let parents = commit_parents(&text);
    let old_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
        resolve_tree_from_source(parent)?
    } else {
        BTreeMap::new()
    };
    let new_tree = resolve_tree_from_source(&commit_hash)?;

    let mut all_paths = BTreeSet::new();
    for p in old_tree.keys() {
        all_paths.insert(p.clone());
    }
    for p in new_tree.keys() {
        all_paths.insert(p.clone());
    }

    for path in all_paths {
        let old_bytes = if let Some((hash, _)) = old_tree.get(&path) {
            read_object(&hex::encode(hash))?.1
        } else {
            Vec::new()
        };
        let new_bytes = if let Some((hash, _)) = new_tree.get(&path) {
            read_object(&hex::encode(hash))?.1
        } else {
            Vec::new()
        };

        if old_bytes == new_bytes {
            continue;
        }

        let old_str = String::from_utf8_lossy(&old_bytes);
        let new_str = String::from_utf8_lossy(&new_bytes);
        let old_lines: Vec<&str> = if old_bytes.is_empty() { Vec::new() } else { old_str.lines().collect() };
        let new_lines: Vec<&str> = if new_bytes.is_empty() { Vec::new() } else { new_str.lines().collect() };
        let old_label = format!("a/{}", path);
        let new_label = format!("b/{}", path);
        let formatted = format_diff_output(&path, &old_lines, &new_lines, &old_label, &new_label);
        print!("{}", formatted);
    }

    Ok(())
}