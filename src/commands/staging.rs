use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Context;
use crate::helpers::{collect_files, flatten_tree, hash_content, normalize_path, tree_hash_of_commit, write_object};
use crate::index::{build_entry, read_index, write_index};
use crate::refs;

pub fn add(paths: Vec<PathBuf>) -> anyhow::Result<()> {
    let mut entries = read_index().unwrap_or_default();
    let mut files_to_add: Vec<PathBuf> = Vec::new();
    for path in paths {
        collect_files(&path, &mut files_to_add)?;
    }

    if files_to_add.is_empty() {
        println!("nothing specified, nothing added.");
        return Ok(());
    }

    let mut added = 0;
    let mut modified = 0;
    let mut unchanged = 0;

    for file_path in files_to_add {
        let content = fs::read(&file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        let hash = hash_content("blob", &content);
        let rel_path = normalize_path(&file_path);

        match entries.iter().find(|e| e.path == rel_path) {
            Some(existing) if existing.hash == hash => {
                // Content already matches what's staged -- nothing to do.
                unchanged += 1;
                continue;
            }
            Some(_) => modified += 1,
            None => added += 1,
        }

        // Only now actually write the object, since we know it's new content.
        write_object("blob", &content)?;

        let metadata = fs::metadata(&file_path)?;
        let new_entry = build_entry(&rel_path, hash, &metadata);

        entries.retain(|e| e.path != rel_path);
        entries.push(new_entry);
    }

    if added == 0 && modified == 0 {
        println!(
            "nothing to add: {} file(s) already up to date, no modifications found",
            unchanged
        );
        return Ok(());
    }

    write_index(&mut entries)?;

    let mut summary = Vec::new();
    if added > 0 {
        summary.push(format!("{} new", added));
    }
    if modified > 0 {
        summary.push(format!("{} modified", modified));
    }
    if unchanged > 0 {
        summary.push(format!("{} unchanged", unchanged));
    }
    println!("Staged files to index ({}).", summary.join(", "));

    Ok(())
}

pub fn status() -> anyhow::Result<()> {
    let head_state = refs::resolve_head()?;

    let mut no_commits_yet = false;
    let mut head_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();

    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            println!("On branch {}", branch_name);
            let ref_path = format!("refs/heads/{}", branch_name);
            if let Some(commit_hash) = refs::read_ref(&ref_path)? {
                let tree_hash = tree_hash_of_commit(&commit_hash)?;
                flatten_tree(&tree_hash, "", &mut head_tree)?;
            } else {
                no_commits_yet = true;
                println!("\nNo commits yet");
            }
        }
        refs::HeadState::Detached(hash) => {
            println!("HEAD detached at {}", &hash[..7]);
            let tree_hash = tree_hash_of_commit(hash)?;
            flatten_tree(&tree_hash, "", &mut head_tree)?;
        }
    }

    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, ([u8; 20], u32)> = index_entries
        .iter()
        .map(|e| (e.path.clone(), (e.hash, e.mode)))
        .collect();

    let mut staged_new = Vec::new();
    let mut staged_modified = Vec::new();
    let mut staged_deleted = Vec::new();

    for (path, (hash, _mode)) in &index_map {
        match head_tree.get(path) {
            None => staged_new.push(path.clone()),
            Some((head_hash, _)) if head_hash != hash => staged_modified.push(path.clone()),
            _ => {}
        }
    }
    for path in head_tree.keys() {
        if !index_map.contains_key(path) {
            staged_deleted.push(path.clone());
        }
    }

    let mut working_files = Vec::new();
    collect_files(Path::new("."), &mut working_files)?;

    let mut working_map: BTreeMap<String, [u8; 20]> = BTreeMap::new();
    for file_path in &working_files {
        let rel_path = normalize_path(file_path);
        let content = fs::read(file_path).with_context(|| format!("Failed to read {}", file_path.display()))?;
        working_map.insert(rel_path, hash_content("blob", &content));
    }

    for path in index_map.keys() {
        if !working_map.contains_key(path) {
            let path_obj = Path::new(path);
            if path_obj.exists() && path_obj.is_file() {
                if let Ok(content) = fs::read(path_obj) {
                    working_map.insert(path.clone(), hash_content("blob", &content));
                }
            }
        }
    }

    let mut unstaged_modified = Vec::new();
    let mut unstaged_deleted = Vec::new();
    let mut untracked = Vec::new();

    for (path, hash) in &working_map {
        match index_map.get(path) {
            None => untracked.push(path.clone()),
            Some((idx_hash, _)) if idx_hash != hash => unstaged_modified.push(path.clone()),
            _ => {}
        }
    }
    for path in index_map.keys() {
        if !working_map.contains_key(path) {
            unstaged_deleted.push(path.clone());
        }
    }

    let has_staged = !staged_new.is_empty() || !staged_modified.is_empty() || !staged_deleted.is_empty();
    let has_unstaged = !unstaged_modified.is_empty() || !unstaged_deleted.is_empty();
    let has_untracked = !untracked.is_empty();
    let mut need_leading_blank = no_commits_yet;

    if has_staged {
        if need_leading_blank { println!(); }
        println!("Changes to be committed:");
        for p in &staged_new { println!("\tnew file:   {}", p); }
        for p in &staged_modified { println!("\tmodified:   {}", p); }
        for p in &staged_deleted { println!("\tdeleted:    {}", p); }
        println!();
        need_leading_blank = false;
    }

    if has_unstaged {
        if need_leading_blank { println!(); }
        println!("Changes not staged for commit:");
        for p in &unstaged_modified { println!("\tmodified:   {}", p); }
        for p in &unstaged_deleted { println!("\tdeleted:    {}", p); }
        println!();
        need_leading_blank = false;
    }

    if has_untracked {
        if need_leading_blank { println!(); }
        println!("Untracked files:");
        for p in &untracked { println!("\t{}", p); }
        println!();
        need_leading_blank = false;
    }

    if !has_staged {
        if need_leading_blank { println!(); }
        if no_commits_yet && !has_unstaged && !has_untracked {
            println!("nothing to commit (create/copy files and use 'add' to track)");
        } else if has_unstaged || has_untracked {
            println!("no changes added to commit (use 'add' to track or stage changes)");
        } else {
            println!("nothing to commit, working tree clean");
        }
    }

    Ok(())
}

pub fn rm(files: Vec<PathBuf>, force: bool, cached: bool, recursive: bool) -> anyhow::Result<()> {
    if files.is_empty() {
        anyhow::bail!("fatal: No pathspec was given. Which files should I remove?");
    }

    let mut index_entries = read_index().unwrap_or_default();
    let head_tree = if let Some(commit_hash) = refs::resolve_head_commit()? {
        let tree_hash = tree_hash_of_commit(&commit_hash)?;
        let mut map = BTreeMap::new();
        flatten_tree(&tree_hash, "", &mut map)?;
        Some(map)
    } else {
        None
    };

    let mut paths_to_remove: Vec<String> = Vec::new();

    for path_buf in &files {
        let rel_path = normalize_path(path_buf);

        let matching_entries: Vec<String> = index_entries
            .iter()
            .filter_map(|e| {
                if e.path == rel_path {
                    Some(e.path.clone())
                } else if recursive && e.path.starts_with(&format!("{}/", rel_path)) {
                    Some(e.path.clone())
                } else {
                    None
                }
            })
            .collect();

        if matching_entries.is_empty() {
            let has_subentries = index_entries
                .iter()
                .any(|e| e.path.starts_with(&format!("{}/", rel_path)));

            if has_subentries && !recursive {
                anyhow::bail!("fatal: not removing '{}' recursively without -r", rel_path);
            } else {
                anyhow::bail!("fatal: pathspec '{}' did not match any files", rel_path);
            }
        }

        for path in matching_entries {
            if !paths_to_remove.contains(&path) {
                paths_to_remove.push(path);
            }
        }
    }

    if !force {
        let mut staged_files = Vec::new();
        let mut modified_files = Vec::new();

        for path in &paths_to_remove {
            let entry = index_entries.iter().find(|e| &e.path == path);
            let idx_hash = entry.map(|e| e.hash);

            let head_hash = head_tree.as_ref().and_then(|t| t.get(path)).map(|(h, _)| *h);

            let index_staged = match (idx_hash, head_hash) {
                (Some(ih), Some(hh)) => ih != hh,
                (Some(_), None) => true,
                _ => false,
            };

            let disk_modified = if !cached && !index_staged && Path::new(path).exists() {
                if let Ok(content) = fs::read(path) {
                    let disk_hash = hash_content("blob", &content);
                    idx_hash.map(|ih| disk_hash != ih).unwrap_or(false)
                } else {
                    false
                }
            } else {
                false
            };

            // Mirror git's own precedence: a file that differs from HEAD is reported
            // as having staged changes, even if it also has further working-tree edits.
            if index_staged {
                staged_files.push(path.clone());
            } else if disk_modified {
                modified_files.push(path.clone());
            }
        }

        if !staged_files.is_empty() || !modified_files.is_empty() {
            let mut msg = String::new();

            if !staged_files.is_empty() {
                msg.push_str("error: the following file has changes staged in the index:\n");
                for file in &staged_files {
                    msg.push_str(&format!("    {}\n", file));
                }
            }
            if !modified_files.is_empty() {
                msg.push_str("error: the following file has local modifications:\n");
                for file in &modified_files {
                    msg.push_str(&format!("    {}\n", file));
                }
            }
            msg.push_str("(use --cached to keep the file, or -f to force removal)");
            anyhow::bail!(msg);
        }
    }

    for path in &paths_to_remove {
        index_entries.retain(|e| &e.path != path);

        if !cached {
            let path_obj = Path::new(path);
            if path_obj.exists() {
                fs::remove_file(path_obj)
                    .with_context(|| format!("failed to remove file '{}'", path))?;

                let mut parent = path_obj.parent();
                while let Some(p) = parent {
                    if p == Path::new("") || p == Path::new(".") {
                        break;
                    }
                    if p.exists() {
                        if fs::read_dir(p)?.next().is_none() {
                            fs::remove_dir(p)?;
                        } else {
                            break;
                        }
                    }
                    parent = p.parent();
                }
            }
        }

        println!("rm '{}'", path);
    }

    write_index(&mut index_entries)?;

    Ok(())
}