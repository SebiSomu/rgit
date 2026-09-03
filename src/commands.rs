use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::helpers::*;
use crate::refs;
use crate::index::*;
use crate::objects::*;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

pub fn init() -> Result<()> {
    fs::create_dir_all(".git/objects")?;
    fs::create_dir_all(".git/refs/heads")?;
    fs::write(".git/HEAD", "ref: refs/heads/main\n")?;
    println!("Initialized empty git repository in .git/");
    Ok(())
}

pub fn hash_object(write: bool, file: PathBuf) -> Result<()> {
    let content = fs::read(&file)?;
    if write {
        println!("{}", write_object("blob", &content)?);
    } else {
        let header = format!("blob {}\0", content.len());
        let mut store = header.into_bytes();
        store.extend_from_slice(&content);
        let mut hasher = Sha1::new();
        hasher.update(&store);
        println!("{}", hex::encode(hasher.finalize()));
    }
    Ok(())
}

pub fn cat_file(pretty_print: bool, object_hash: String) -> Result<()> {
    let dir = &object_hash[0..2];
    let file_name = &object_hash[2..];
    let path = format!(".git/objects/{}/{}", dir, file_name);

    let compressed_data = fs::read(&path).context("Failed to read object file")?;
    let mut decoder = ZlibDecoder::new(&compressed_data[..]);
    let mut decompressed_data = Vec::new();
    decoder.read_to_end(&mut decompressed_data)?;

    let null_pos = decompressed_data.iter().position(|&b| b == 0).context("Invalid Git object")?;

    if pretty_print {
        let content = &decompressed_data[null_pos + 1..];
        let text = String::from_utf8_lossy(content);
        println!("{}", text);
    }
    Ok(())
}

pub fn write_tree() -> Result<()> {
    let entries = read_index().unwrap_or_default();
    let tree_hash = write_tree_from_index_prefix(&entries, "")?;
    println!("{}", tree_hash);
    Ok(())
}

fn write_tree_from_index_prefix(entries: &[IndexEntry], prefix: &str) -> Result<String> {
    let prefix_with_slash = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", prefix)
    };

    let mut direct_files: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
    let mut subdirs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for entry in entries {
        if !prefix_with_slash.is_empty() && !entry.path.starts_with(&prefix_with_slash) {
            continue;
        }

        let rel = if prefix_with_slash.is_empty() {
            entry.path.as_str()
        } else {
            &entry.path[prefix_with_slash.len()..]
        };

        if let Some(slash_idx) = rel.find('/') {
            let subdir_name = &rel[..slash_idx];
            subdirs.insert(subdir_name.to_string());
        } else {
            direct_files.insert(rel.to_string(), (entry.hash, entry.mode));
        }
    }

    let mut tree_entries: Vec<(String, Vec<u8>)> = Vec::new();

    for (name, (hash, mode)) in direct_files {
        let mut entry_bytes = Vec::new();
        let mode_str = format!("{:o} ", mode);
        entry_bytes.extend_from_slice(mode_str.as_bytes());
        entry_bytes.extend_from_slice(name.as_bytes());
        entry_bytes.push(0);
        entry_bytes.extend_from_slice(&hash);
        tree_entries.push((name, entry_bytes));
    }

    for subdir in subdirs {
        let sub_prefix = if prefix.is_empty() {
            subdir.clone()
        } else {
            format!("{}/{}", prefix, subdir)
        };
        let sub_tree_hash_hex = write_tree_from_index_prefix(entries, &sub_prefix)?;
        let sub_tree_hash = hex::decode(sub_tree_hash_hex)?;

        let mut entry_bytes = Vec::new();
        entry_bytes.extend_from_slice(b"40000 ");
        entry_bytes.extend_from_slice(subdir.as_bytes());
        entry_bytes.push(0);
        entry_bytes.extend_from_slice(&sub_tree_hash);
        tree_entries.push((subdir, entry_bytes));
    }

    tree_entries.sort_by(|(name_a, _), (name_b, _)| name_a.cmp(name_b));

    let mut tree_content = Vec::new();
    for (_, bytes) in tree_entries {
        tree_content.extend_from_slice(&bytes);
    }

    write_object("tree", &tree_content)
}

pub fn ls_tree(name_only: bool, tree_hash: String) -> Result<()> {
    let (object_type, content) = read_object(&tree_hash)?;
    if object_type != "tree" {
        anyhow::bail!("Not a tree object: {}", tree_hash);
    }

    let mut pos = 0;
    while pos < content.len() {
        let space_pos = content[pos..].iter().position(|&b| b == b' ').context("Missing mode separator")? + pos;
        let mode = String::from_utf8_lossy(&content[pos..space_pos]).to_string();

        let null_pos = content[space_pos..].iter().position(|&b| b == 0).context("Missing terminator")? + space_pos;
        let name = String::from_utf8_lossy(&content[space_pos + 1..null_pos]).to_string();

        let hash_start = null_pos + 1;
        let hash_end = hash_start + 20;
        let sha_hex = hex::encode(&content[hash_start..hash_end]);

        if name_only {
            println!("{}", name);
        } else {
            let entry_type = if mode == "40000" { "tree" } else { "blob" };
            println!("{:0>6} {} {}\t{}", mode, entry_type, sha_hex, name);
        }
        pos = hash_end;
    }
    Ok(())
}

fn build_commit(tree_hash: String, parent_hash: Option<String>, message: &str) -> Result<String> {
    let mut content = format!("tree {}\n", tree_hash);
    if let Some(parent) = parent_hash {
        content.push_str(&format!("parent {}\n", parent));
    }

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let author = "rgit <rgit@example.com>";

    content.push_str(&format!("author {} {} +0000\n", author, timestamp));
    content.push_str(&format!("committer {} {} +0000\n", author, timestamp));
    content.push_str(&format!("\n{}\n", message));

    write_object("commit", content.as_bytes())
}

pub fn commit_tree(tree_hash: String, parent_hash: Option<String>, message: String) -> Result<()> {
    println!("{}", build_commit(tree_hash, parent_hash, &message)?);
    Ok(())
}

pub fn commit(message: String) -> Result<()> {
    let entries = read_index().unwrap_or_default();
    let tree_hash = write_tree_from_index_prefix(&entries, "")?;
    let head_state = refs::resolve_head()?;
    let parent_hash = refs::resolve_head_commit()?;

    let second_parent = if Path::new(".git/MERGE_HEAD").exists() {
        let content = fs::read_to_string(".git/MERGE_HEAD")?;
        Some(content.trim().to_string())
    } else {
        None
    };

    if let (Some(parent), None) = (&parent_hash, &second_parent) {
        if tree_hash_of_commit(parent)? == tree_hash {
            anyhow::bail!("nothing to commit (working tree matches HEAD)");
        }
    }

    let commit_hash = if let (Some(p1), Some(p2)) = (&parent_hash, &second_parent) {
        build_merge_commit(tree_hash, p1, p2, &message)?
    } else {
        build_commit(tree_hash, parent_hash.clone(), &message)?
    };

    if Path::new(".git/MERGE_HEAD").exists() {
        let _ = fs::remove_file(".git/MERGE_HEAD");
    }
    if Path::new(".git/MERGE_MSG").exists() {
        let _ = fs::remove_file(".git/MERGE_MSG");
    }

    let short_hash = &commit_hash[..7];

    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            let ref_path = format!("refs/heads/{}", branch_name);
            refs::write_ref(&ref_path, &commit_hash)?;
            if parent_hash.is_none() {
                println!("[{} (root-commit) {}] {}", branch_name, short_hash, message);
            } else {
                println!("[{} {}] {}", branch_name, short_hash, message);
            }
        }
        refs::HeadState::Detached(_) => {
            refs::set_head_detached(&commit_hash)?;
            if parent_hash.is_none() {
                println!("[(detached HEAD) (root-commit) {}] {}", short_hash, message);
            } else {
                println!("[(detached HEAD) {}] {}", short_hash, message);
            }
            eprintln!("warning: You are in a detached HEAD state.");
        }
    }

    Ok(())
}

pub fn log(oneline: bool) -> Result<()> {
    let head_state = refs::resolve_head()?;

    let (label, mut current_hash) = match &head_state {
        refs::HeadState::Branch(b) => {
            let ref_path = format!("refs/heads/{}", b);
            (b.clone(), refs::read_ref(&ref_path)?)
        }
        refs::HeadState::Detached(h) => ("HEAD".to_string(), Some(h.clone())),
    };

    if current_hash.is_none() {
        anyhow::bail!(
            "fatal: your current branch '{}' does not have any commits yet",
            label
        );
    }

    let mut first = true;

    while let Some(hash) = current_hash {
        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            anyhow::bail!("{} is not a commit object", hash);
        }

        let text = String::from_utf8_lossy(&content);
        let mut parent: Option<String> = None;
        let mut author_line: Option<String> = None;
        let mut message_lines: Vec<String> = Vec::new();
        let mut in_message = false;

        for line in text.lines() {
            if in_message {
                message_lines.push(line.to_string());
            } else if line.is_empty() {
                in_message = true;
            } else if let Some(p) = line.strip_prefix("parent ") {
                parent = Some(p.to_string());
            } else if let Some(a) = line.strip_prefix("author ") {
                author_line = Some(a.to_string());
            }
        }

        if oneline {
            let first_line = message_lines.first().map(|s| s.as_str()).unwrap_or("");
            println!("{} {}", &hash[..7], first_line);
        } else {
            if !first {
                println!();
            }

            println!("commit {}", hash);

            if let Some(author) = &author_line {
                println!("Author: {}", author);
            }

            println!();
            for line in &message_lines {
                println!("    {}", line);
            }
        }

        first = false;
        current_hash = parent;
    }

    Ok(())
}

pub fn add(paths: Vec<PathBuf>) -> Result<()> {
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

pub fn status() -> Result<()> {
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

pub fn branch(name: Option<String>, delete: bool, force_delete: bool, rename: Option<String>) -> Result<()> {
    if let Some(new_name) = rename {
        let old_name = match name {
            Some(ref n) => n.as_str().to_string(),
            None => {
                let ref_path = refs::current_branch_ref()?;
                ref_path
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&ref_path)
                    .to_string()
            }
        };
        refs::rename_branch(&old_name, &new_name)?;
        println!("Renamed branch '{}' to '{}'.", old_name, new_name);
        return Ok(());
    }

    if delete || force_delete {
        let branch_name = name.ok_or_else(|| {
            anyhow::anyhow!("fatal: branch name required for delete")
        })?;
        let head = refs::resolve_head()?;
        if let refs::HeadState::Branch(ref current) = head {
            if current == &branch_name {
                anyhow::bail!(
                    "error: Cannot delete branch '{}' checked out locally.",
                    branch_name
                );
            }
        }

        if delete && !force_delete {
            let branch_ref = format!("refs/heads/{}", branch_name);
            if let Some(branch_tip) = refs::read_ref(&branch_ref)? {
                let head_commit = match &head {
                    refs::HeadState::Branch(b) => {
                        let r = format!("refs/heads/{}", b);
                        refs::read_ref(&r)?
                    }
                    refs::HeadState::Detached(h) => Some(h.clone()),
                };

                let merged = if let Some(head_hash) = head_commit {
                    is_reachable(&head_hash, &branch_tip)?
                } else {
                    false
                };

                if !merged {
                    anyhow::bail!("error: The branch '{}' is not fully merged.\n If you are sure you want to delete it, run 'branch -D {}'.", branch_name, branch_name);
                }
            }
        }

        refs::delete_branch(&branch_name)?;
        println!("Deleted branch {}.", branch_name);
        return Ok(());
    }

    match name {
        None => {
            let head_state = refs::resolve_head()?;
            let current_branch = match &head_state {
                refs::HeadState::Branch(b) => Some(b.as_str()),
                refs::HeadState::Detached(_) => None,
            };

            let branches = refs::list_branches()?;

            if branches.is_empty() {
                if let Some(name) = current_branch {
                    println!("* {}", name);
                }
            } else {
                for branch in &branches {
                    if Some(branch.as_str()) == current_branch {
                        println!("* {}", branch);
                    } else {
                        println!("  {}", branch);
                    }
                }
            }
        }
        Some(branch_name) => {
            let ref_path = refs::current_branch_ref()?;
            let commit_hash = refs::read_ref(&ref_path)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "fatal: not a valid object name: '{}' has no commits yet",
                    ref_path.strip_prefix("refs/heads/").unwrap_or(&ref_path)
                )
            })?;
            refs::create_branch(&branch_name, &commit_hash)?;
            println!("Created branch '{}' at {}", branch_name, &commit_hash[..7]);
        }
    }

    Ok(())
}

pub fn switch(branch: String, create: bool, detach: bool, force: bool) -> Result<()> {
    if create {
        let commit_hash = refs::resolve_head_commit()?.ok_or_else(|| {
            anyhow::anyhow!("fatal: cannot create branch — no commits yet")
        })?;
        refs::create_branch(&branch, &commit_hash)?;
    }

    let target_commit = if detach {
        let branch_ref = format!("refs/heads/{}", branch);
        if let Some(hash) = refs::read_ref(&branch_ref)? {
            hash
        } else {
            match read_object(&branch) {
                Ok((obj_type, _)) if obj_type == "commit" => branch.clone(),
                Ok((obj_type, _)) => {
                    anyhow::bail!("fatal: '{}' is not a commit (it is a {})", branch, obj_type);
                }
                Err(_) => {
                    anyhow::bail!("fatal: '{}' is not a valid object hash or branch name", branch);
                }
            }
        }
    } else {
        let target_ref = format!("refs/heads/{}", branch);
        match refs::read_ref(&target_ref)? {
            Some(hash) => hash,
            None => anyhow::bail!("fatal: invalid reference: {}", branch),
        }
    };

    if !detach {
        let current_head = refs::resolve_head()?;
        if let refs::HeadState::Branch(ref current_branch) = current_head {
            if current_branch == &branch {
                println!("Already on '{}'", branch);
                return Ok(());
            }
        }
    }

    let target_tree_hash = tree_hash_of_commit(&target_commit)?;
    let mut target_tree = BTreeMap::new();
    flatten_tree(&target_tree_hash, "", &mut target_tree)?;

    let mut head_tree = BTreeMap::new();
    if let Some(current_commit) = refs::resolve_head_commit()? {
        let current_tree_hash = tree_hash_of_commit(&current_commit)?;
        flatten_tree(&current_tree_hash, "", &mut head_tree)?;
    }

    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, [u8; 20]> = index_entries
        .iter()
        .map(|e| (e.path.clone(), e.hash))
        .collect();

    if !force {
        check_switch_safety(&target_tree, &head_tree, &index_map)?;
    }

    sync_working_tree(&target_tree, &head_tree, &index_map)?;
    update_index_from_tree(&target_tree, &head_tree)?;

    if detach {
        refs::set_head_detached(&target_commit)?;
        println!("HEAD is now at {} (detached)", &target_commit[..7]);
        eprintln!("warning: You are in a detached HEAD state.");
        eprintln!("  You can look around, make experimental commits, or switch -c <branch> to keep them.");
    } else if create {
        refs::set_head(&branch)?;
        println!("Switched to a new branch '{}'", branch);
    } else {
        refs::set_head(&branch)?;
        println!("Switched to branch '{}'", branch);
    }

    Ok(())
}

pub fn checkout(target: Option<String>, create_branch: Option<String>, detach: bool, force: bool) -> Result<()> {
    if create_branch.is_none() && target.is_none() {
        anyhow::bail!("fatal: you must specify a branch or commit to checkout");
    }

    if let Some(new_branch) = create_branch {
        let commit_hash = if let Some(start_point) = target {
            let branch_ref = format!("refs/heads/{}", start_point);
            if let Some(hash) = refs::read_ref(&branch_ref)? {
                hash
            } else {
                match read_object(&start_point) {
                    Ok((obj_type, _)) if obj_type == "commit" => start_point,
                    Ok((obj_type, _)) => {
                        anyhow::bail!("fatal: '{}' is not a commit (it is a {})", start_point, obj_type);
                    }
                    Err(_) => {
                        anyhow::bail!("fatal: '{}' is not a valid object hash or branch name", start_point);
                    }
                }
            }
        } else {
            refs::resolve_head_commit()?.ok_or_else(|| {
                anyhow::anyhow!("fatal: cannot create branch — no commits yet")
            })?
        };

        refs::create_branch(&new_branch, &commit_hash)?;
        switch(new_branch, false, false, force)?;
    } else if let Some(t) = target {
        let is_branch = {
            let branch_ref = format!("refs/heads/{}", t);
            refs::read_ref(&branch_ref)?.is_some()
        };
        let should_detach = detach || !is_branch;
        switch(t, false, should_detach, force)?;
    }

    Ok(())
}

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
                            ctime_secs: 0, ctime_nsecs: 0,
                            mtime_secs: 0, mtime_nsecs: 0,
                            dev: 0, ino: 0,
                            mode,
                            uid: 0, gid: 0,
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

pub fn diff(staged: bool, commit_a: Option<String>, commit_b: Option<String>, paths: Vec<PathBuf>) -> Result<()> {
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

        let mut all_paths = std::collections::BTreeSet::new();
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

        let mut all_paths = std::collections::BTreeSet::new();
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

        let mut all_paths = std::collections::BTreeSet::new();
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

        let mut all_paths = std::collections::BTreeSet::new();
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

pub fn merge(branch: String) -> Result<()> {
    let head_state = refs::resolve_head()?;
    let current_branch = match head_state {
        refs::HeadState::Branch(b) => b,
        refs::HeadState::Detached(_) => {
            anyhow::bail!("fatal: You are in 'detached HEAD' state. Please switch to a branch before merging.");
        }
    };

    let our_commit = refs::resolve_head_commit()?.ok_or_else(|| {
        anyhow::anyhow!("fatal: HEAD has no commits yet")
    })?;

    let their_commit = resolve_commit_from_source(&branch)?;

    if our_commit == their_commit {
        println!("Already up to date.");
        return Ok(());
    }

    if is_reachable(&our_commit, &their_commit)? {
        println!("Already up to date.");
        return Ok(());
    }

    if is_reachable(&their_commit, &our_commit)? {
        let target_tree = resolve_tree_from_source(&their_commit)?;
        let head_tree = resolve_tree_from_source(&our_commit)?;
        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

        check_switch_safety(&target_tree, &head_tree, &index_map)?;
        sync_working_tree(&target_tree, &head_tree, &index_map)?;
        update_index_from_tree(&target_tree, &head_tree)?;

        let branch_ref = format!("refs/heads/{}", current_branch);
        refs::write_ref(&branch_ref, &their_commit)?;

        println!("Updating {}..{}", &our_commit[..7], &their_commit[..7]);
        println!("Fast-forward");
        return Ok(());
    }

    let base_commit = find_merge_base(&our_commit, &their_commit)?.ok_or_else(|| {
        anyhow::anyhow!("fatal: refusing to merge unrelated histories")
    })?;

    let base_tree = resolve_tree_from_source(&base_commit)?;
    let our_tree = resolve_tree_from_source(&our_commit)?;
    let their_tree = resolve_tree_from_source(&their_commit)?;

    let mut all_paths = std::collections::BTreeSet::new();
    for p in base_tree.keys() { all_paths.insert(p.clone()); }
    for p in our_tree.keys() { all_paths.insert(p.clone()); }
    for p in their_tree.keys() { all_paths.insert(p.clone()); }

    let mut merged_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
    let mut conflicts: Vec<MergeConflict> = Vec::new();

    for path in all_paths {
        let base_entry = base_tree.get(&path);
        let our_entry = our_tree.get(&path);
        let their_entry = their_tree.get(&path);

        if our_entry == their_entry {
            if let Some(entry) = our_entry {
                merged_tree.insert(path, *entry);
            }
        } else if our_entry == base_entry {
            if let Some(entry) = their_entry {
                merged_tree.insert(path, *entry);
            }
        } else if their_entry == base_entry {
            if let Some(entry) = our_entry {
                merged_tree.insert(path, *entry);
            }
        } else {
            let our_bytes = if let Some(our) = our_entry {
                read_object(&hex::encode(our.0))?.1
            } else {
                Vec::new()
            };
            let their_bytes = if let Some(their) = their_entry {
                read_object(&hex::encode(their.0))?.1
            } else {
                Vec::new()
            };
            let base_bytes = if let Some(base) = base_entry {
                read_object(&hex::encode(base.0))?.1
            } else {
                Vec::new()
            };

            conflicts.push(MergeConflict {
                path,
                base: base_bytes,
                ours: our_bytes,
                theirs: their_bytes,
            });
        }
    }

    if !conflicts.is_empty() {
        for conflict in &conflicts {
            let conflict_content = generate_conflict_markers(&conflict.ours, &conflict.theirs, &branch);
            if let Some(parent) = Path::new(&conflict.path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(&conflict.path, &conflict_content)?;
            println!("CONFLICT (content): Merge conflict in {}", conflict.path);
        }

        for (path, (hash, _mode)) in &merged_tree {
            let (_, content) = read_object(&hex::encode(hash))?;
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(path, &content)?;
        }

        fs::write(".git/MERGE_HEAD", format!("{}\n", their_commit))?;
        fs::write(".git/MERGE_MSG", format!("Merge branch '{}'\n", branch))?;

        println!("Automatic merge failed; fix conflicts and then commit the result.");
        return Ok(());
    }

    for path in our_tree.keys() {
        if !merged_tree.contains_key(path) {
            let path_obj = Path::new(path);
            if path_obj.exists() {
                let _ = fs::remove_file(path_obj);
            }
        }
    }

    let mut new_index_entries = Vec::new();
    for (path, (hash, _mode)) in &merged_tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, &content)?;

        let metadata = fs::metadata(path)?;
        let entry = build_entry(path, *hash, &metadata);
        new_index_entries.push(entry);
    }

    write_index(&mut new_index_entries)?;

    let tree_hash = write_tree_from_index_prefix(&new_index_entries, "")?;
    let merge_msg = format!("Merge branch '{}'", branch);
    let commit_hash = build_merge_commit(tree_hash, &our_commit, &their_commit, &merge_msg)?;
    let short_hash = &commit_hash[..7];

    let branch_ref = format!("refs/heads/{}", current_branch);
    refs::write_ref(&branch_ref, &commit_hash)?;

    println!("Merge made by the 'ort' strategy.");
    println!("[{} {}] {}", current_branch, short_hash, merge_msg);

    Ok(())
}

pub fn rm(files: Vec<PathBuf>, force: bool, cached: bool, recursive: bool) -> Result<()> {
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

enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

/// Force-overwrites the working directory to exactly match `target_tree`, discarding
/// any uncommitted changes. Deletes files that were tracked (per `tracked_paths`) but
/// are absent from the target.
///
/// Unlike `sync_working_tree` (used by `switch`/`checkout`), every file present in the
/// target is rewritten unconditionally rather than only when it differs from the old
/// HEAD tree — `reset --hard` must discard local modifications even when their content
/// happens to match the previous HEAD.
// Used by `reset --hard`.
fn hard_reset_working_tree(
    target_tree: &BTreeMap<String, ([u8; 20], u32)>,
    tracked_paths: &std::collections::BTreeSet<String>,
) -> Result<()> {
    for path in tracked_paths {
        if target_tree.contains_key(path) {
            continue;
        }

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

    for (path, (hash, mode)) in target_tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, &content).with_context(|| format!("failed to write '{}'", path))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = if (*mode & 0o111) != 0 { 0o755 } else { 0o644 };
            fs::set_permissions(path, fs::Permissions::from_mode(file_mode))?;
        }
    }

    Ok(())
}

/// Builds a fresh set of index entries describing exactly `target_tree`, independent of
/// whatever the index previously contained. When `read_disk_metadata` is true (after a
/// hard reset has just written every file to disk), real stat info is captured via
/// `build_entry`; otherwise (a mixed reset, which never touches the working tree) a
/// placeholder stat entry is used, mirroring the convention already used by `restore`.
// Used by `reset --mixed` and `reset --hard`.
fn build_index_entries_for_tree(
    target_tree: &BTreeMap<String, ([u8; 20], u32)>,
    read_disk_metadata: bool,
) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::with_capacity(target_tree.len());

    for (path, (hash, mode)) in target_tree {
        if read_disk_metadata {
            if let Ok(metadata) = fs::metadata(path) {
                let mut entry = build_entry(path, *hash, &metadata);
                entry.mode = *mode;
                entries.push(entry);
                continue;
            }
        }

        let (_, content) = read_object(&hex::encode(hash))?;
        entries.push(IndexEntry {
            ctime_secs: 0,
            ctime_nsecs: 0,
            mtime_secs: 0,
            mtime_nsecs: 0,
            dev: 0,
            ino: 0,
            mode: *mode,
            uid: 0,
            gid: 0,
            size: content.len() as u32,
            hash: *hash,
            path: path.clone(),
        });
    }

    Ok(entries)
}

/// Returns the first line of a commit's message body (its "subject line").
// Used by `reset --hard` to print the familiar `HEAD is now at <hash> <subject>` line.
fn commit_subject_line(commit_hash: &str) -> Result<String> {
    let (object_type, content) = read_object(commit_hash)?;
    if object_type != "commit" {
        anyhow::bail!("{} is not a commit object", commit_hash);
    }

    let text = String::from_utf8_lossy(&content);
    let mut in_message = false;
    for line in text.lines() {
        if in_message {
            return Ok(line.to_string());
        } else if line.is_empty() {
            in_message = true;
        }
    }

    Ok(String::new())
}

/// Prints the `Unstaged changes after reset:` summary that `git reset` shows after a
/// mixed reset, listing what changed between the previous index and the new one using
/// the familiar `A`/`M`/`D` status letters.
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

fn reset_paths(source: Option<&str>, paths: &[PathBuf]) -> Result<()> {
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

pub fn reset(commit: Option<String>, soft: bool, _mixed: bool, hard: bool, paths: Vec<PathBuf>) -> Result<()> {
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

/// Controls which untracked entries `clean` considers, mirroring git's
/// `-x` / `-X` / default distinction around `.gitignore`.
enum CleanMode {
    /// Default: consider only untracked files that are NOT ignored.
    Normal,
    /// `-x`: consider untracked files whether ignored or not.
    IncludeIgnored,
    /// `-X`: consider ONLY files that are ignored.
    IgnoredOnly,
}

impl CleanMode {
    fn wants(&self, ignored: bool) -> bool {
        match self {
            CleanMode::Normal => !ignored,
            CleanMode::IncludeIgnored => true,
            CleanMode::IgnoredOnly => ignored,
        }
    }
}

/// Removes files and directories from the working tree that are not tracked by the
/// index, similar to `git clean`. Requires either `dry_run` or `force`, matching
/// git's `clean.requireForce` default safety behavior.
// Used by the `clean` command.
pub fn clean(dry_run: bool, force: bool, dirs: bool, ignored: bool, only_ignored: bool) -> Result<()> {
    if ignored && only_ignored {
        anyhow::bail!("fatal: -x and -X cannot be used together");
    }

    if !dry_run && !force {
        anyhow::bail!(
            "fatal: clean.requireForce defaults to true and neither -n nor -f given; refusing to clean"
        );
    }

    let mode = if only_ignored {
        CleanMode::IgnoredOnly
    } else if ignored {
        CleanMode::IncludeIgnored
    } else {
        CleanMode::Normal
    };

    let index_entries = read_index().unwrap_or_default();
    let tracked_paths: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();

    // Every ancestor directory of every tracked path, so we can tell whether an
    // untracked directory has tracked content underneath it (and must therefore
    // be descended into rather than removed wholesale).
    let mut tracked_dirs: BTreeSet<String> = BTreeSet::new();
    for path in &tracked_paths {
        let mut components: Vec<&str> = path.split('/').collect();
        components.pop();
        let mut prefix = String::new();
        for comp in components {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(comp);
            tracked_dirs.insert(prefix.clone());
        }
    }

    let mut files_out: Vec<String> = Vec::new();
    let mut dirs_out: Vec<String> = Vec::new();
    let mut rules: Vec<GitIgnoreRule> = Vec::new();

    collect_clean_candidates(
        Path::new("."),
        &mut rules,
        &tracked_paths,
        &tracked_dirs,
        &mode,
        dirs,
        &mut files_out,
        &mut dirs_out,
    )?;

    files_out.sort();
    dirs_out.sort();

    if files_out.is_empty() && dirs_out.is_empty() {
        println!("nothing to clean, working tree already clean");
        return Ok(());
    }

    let verb = if dry_run { "Would remove" } else { "Removing" };

    for dir in &dirs_out {
        if !dry_run {
            fs::remove_dir_all(dir).with_context(|| format!("Failed to remove directory {}", dir))?;
        }
        println!("{} {}/", verb, dir);
    }
    for file in &files_out {
        if !dry_run {
            fs::remove_file(file).with_context(|| format!("Failed to remove {}", file))?;
        }
        println!("{} {}", verb, file);
    }

    Ok(())
}

/// Recursively walks the working tree (skipping `.git`) collecting untracked files
/// and, when `remove_dirs` is set, whole untracked directories, according to `mode`.
/// Mirrors the `.gitignore`-aware traversal used by `collect_files`, but additionally
/// distinguishes tracked vs. untracked content and never descends into a directory
/// it is about to remove wholesale.
// Used by `clean`.
fn collect_clean_candidates(
    path: &Path,
    rules: &mut Vec<GitIgnoreRule>,
    tracked_paths: &BTreeSet<String>,
    tracked_dirs: &BTreeSet<String>,
    mode: &CleanMode,
    remove_dirs: bool,
    files_out: &mut Vec<String>,
    dirs_out: &mut Vec<String>,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to stat {}", path.display()))?;
    let rel_path = normalize_path(path);

    if metadata.is_file() || metadata.file_type().is_symlink() {
        if tracked_paths.contains(&rel_path) {
            return Ok(());
        }
        let ignored = !rel_path.is_empty() && is_path_ignored(rules, &rel_path, false);
        if mode.wants(ignored) {
            files_out.push(rel_path);
        }
        return Ok(());
    }

    if metadata.is_dir() {
        let name_str = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if name_str == ".git" {
            return Ok(());
        }

        let ignored = !rel_path.is_empty() && is_path_ignored(rules, &rel_path, true);
        let has_tracked_content = tracked_dirs.contains(&rel_path);

        // A directory with nothing tracked underneath it can be removed wholesale
        // (when -d is given) instead of being walked entry-by-entry.
        if remove_dirs && !has_tracked_content && !rel_path.is_empty() {
            if mode.wants(ignored) {
                dirs_out.push(rel_path);
                return Ok(());
            }
            if ignored {
                // Ignored directory that the current mode doesn't want: leave it
                // alone entirely, same as git's default refusal to descend into
                // ignored directories.
                return Ok(());
            }
        }

        // Otherwise recurse: either this directory holds tracked content (so we
        // must look for untracked files within it individually), -d wasn't given,
        // or it's ignored but the mode wants us to look inside for matches.
        if ignored && !mode.wants(true) {
            return Ok(());
        }

        let mut current_rules = rules.clone();
        let gitignore_file = path.join(".gitignore");
        if gitignore_file.exists() && gitignore_file.is_file() {
            if let Ok(content) = fs::read_to_string(&gitignore_file) {
                for line in content.lines() {
                    if let Some(rule) = parse_gitignore_line(line, &rel_path) {
                        current_rules.push(rule);
                    }
                }
            }
        }

        let mut dir_entries: Vec<_> = fs::read_dir(path)?.filter_map(Result::ok).collect();
        dir_entries.sort_by_key(|e| e.file_name());

        for entry in dir_entries {
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if entry_name == ".git" {
                continue;
            }
            collect_clean_candidates(
                &entry.path(),
                &mut current_rules,
                tracked_paths,
                tracked_dirs,
                mode,
                remove_dirs,
                files_out,
                dirs_out,
            )?;
        }
    }

    Ok(())
}

/// Replays a single commit's changes (relative to its own parent) onto the current
/// HEAD, similar to `git cherry-pick <commit>`. On a clean apply, creates a new
/// commit with a single parent (the current HEAD) carrying the original message
/// plus a `(cherry picked from commit ...)` trailer. On conflicts, writes conflict
/// markers into the working tree and records `.git/CHERRY_PICK_HEAD` /
/// `.git/CHERRY_PICK_MSG` so `--continue` or `--abort` can finish the job later.
// Used by the `cherry-pick` command.
pub fn cherry_pick(commit: Option<String>, no_commit: bool, cont: bool, abort: bool) -> Result<()> {
    if abort {
        return cherry_pick_abort();
    }
    if cont {
        return cherry_pick_continue();
    }

    let commit_ref = commit.ok_or_else(|| {
        anyhow::anyhow!("fatal: cherry-pick requires a <commit>, or --continue / --abort")
    })?;

    if Path::new(".git/CHERRY_PICK_HEAD").exists() {
        anyhow::bail!(
            "fatal: a cherry-pick is already in progress\n\
             hint: use 'rgit cherry-pick --continue' or 'rgit cherry-pick --abort'"
        );
    }

    let head_state = refs::resolve_head()?;
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;

    let pick_commit = resolve_commit_from_source(&commit_ref)?;
    let (object_type, content) = read_object(&pick_commit)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' does not point to a commit object", commit_ref);
    }
    let text = String::from_utf8_lossy(&content).to_string();

    let parents = commit_parents(&text);
    if parents.len() > 1 {
        anyhow::bail!(
            "error: commit {} is a merge but no -m option was given.\nfatal: cherry-pick failed",
            &pick_commit[..7]
        );
    }

    let original_message = extract_commit_message(&text);
    let final_message = format!("{}\n\n(cherry picked from commit {})", original_message, pick_commit);
    let subject = original_message.lines().next().unwrap_or("").to_string();

    let base_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
        resolve_tree_from_source(parent)?
    } else {
        BTreeMap::new()
    };
    let their_tree = resolve_tree_from_source(&pick_commit)?;
    let our_tree = resolve_tree_from_source(&our_commit)?;

    let (merged_tree, conflicts) = three_way_tree_merge(&base_tree, &our_tree, &their_tree)?;

    if !conflicts.is_empty() {
        for conflict in &conflicts {
            let conflict_content =
                generate_conflict_markers(&conflict.ours, &conflict.theirs, &format!("{}...", &pick_commit[..7]));
            if let Some(parent) = Path::new(&conflict.path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(&conflict.path, &conflict_content)?;
            println!("CONFLICT (content): Merge conflict in {}", conflict.path);
        }

        for (path, (hash, _mode)) in &merged_tree {
            let (_, content) = read_object(&hex::encode(hash))?;
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(path, &content)?;
        }

        fs::write(".git/CHERRY_PICK_HEAD", format!("{}\n", pick_commit))?;
        fs::write(".git/CHERRY_PICK_MSG", format!("{}\n", final_message))?;

        println!("error: could not apply {}... {}", &pick_commit[..7], subject);
        println!("hint: after resolving the conflicts, mark the corrected paths");
        println!("hint: with 'rgit add <paths>' and run 'rgit cherry-pick --continue'");
        println!("hint: (or 'rgit cherry-pick --abort' to give up)");
        return Ok(());
    }

    for path in our_tree.keys() {
        if !merged_tree.contains_key(path) {
            let path_obj = Path::new(path);
            if path_obj.exists() {
                let _ = fs::remove_file(path_obj);
            }
        }
    }

    let mut new_index_entries = Vec::new();
    for (path, (hash, _mode)) in &merged_tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, &content)?;

        let metadata = fs::metadata(path)?;
        let entry = build_entry(path, *hash, &metadata);
        new_index_entries.push(entry);
    }

    write_index(&mut new_index_entries)?;

    let tree_hash = write_tree_from_index_prefix(&new_index_entries, "")?;

    if tree_hash_of_commit(&our_commit)? == tree_hash {
        println!(
            "The previous cherry-pick is now empty, possibly due to conflict resolution."
        );
        println!("nothing to commit");
        return Ok(());
    }

    if no_commit {
        println!("Applied {}... {}", &pick_commit[..7], subject);
        println!("(staged, not committed -- run 'rgit commit -m \"...\"')");
        return Ok(());
    }

    let commit_hash = build_commit(tree_hash, Some(our_commit.clone()), &final_message)?;
    let short_hash = &commit_hash[..7];

    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            let ref_path = format!("refs/heads/{}", branch_name);
            refs::write_ref(&ref_path, &commit_hash)?;
            println!("[{} {}] {}", branch_name, short_hash, subject);
        }
        refs::HeadState::Detached(_) => {
            refs::set_head_detached(&commit_hash)?;
            println!("[(detached HEAD) {}] {}", short_hash, subject);
        }
    }

    Ok(())
}

/// Finishes an in-progress cherry-pick after conflicts have been resolved and
/// staged with `add`. Refuses if unresolved `<<<<<<<` markers remain anywhere in
/// the index's files, then commits with the original message + trailer that was
/// saved to `.git/CHERRY_PICK_MSG` when the conflict was first hit.
// Used by `cherry-pick --continue`.
fn cherry_pick_continue() -> Result<()> {
    if !Path::new(".git/CHERRY_PICK_HEAD").exists() {
        anyhow::bail!("fatal: no cherry-pick in progress");
    }

    let pick_commit = fs::read_to_string(".git/CHERRY_PICK_HEAD")?.trim().to_string();
    let final_message = fs::read_to_string(".git/CHERRY_PICK_MSG")
        .context("fatal: missing .git/CHERRY_PICK_MSG for the in-progress cherry-pick")?
        .trim_end_matches('\n')
        .to_string();

    let entries = read_index().unwrap_or_default();
    for entry in &entries {
        if let Ok(content) = fs::read(&entry.path) {
            if content.windows(7).any(|w| w == b"<<<<<<<") {
                anyhow::bail!(
                    "error: '{}' still has unresolved conflict markers; fix it and 'rgit add' it first",
                    entry.path
                );
            }
        }
    }

    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;

    let tree_hash = write_tree_from_index_prefix(&entries, "")?;
    let commit_hash = build_commit(tree_hash, Some(our_commit), &final_message)?;
    let short_hash = &commit_hash[..7];

    let head_state = refs::resolve_head()?;
    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            let ref_path = format!("refs/heads/{}", branch_name);
            refs::write_ref(&ref_path, &commit_hash)?;
        }
        refs::HeadState::Detached(_) => {
            refs::set_head_detached(&commit_hash)?;
        }
    }

    let _ = fs::remove_file(".git/CHERRY_PICK_HEAD");
    let _ = fs::remove_file(".git/CHERRY_PICK_MSG");

    println!(
        "[{} {}] cherry-pick of {} continued",
        match &head_state {
            refs::HeadState::Branch(b) => b.clone(),
            refs::HeadState::Detached(_) => "(detached HEAD)".to_string(),
        },
        short_hash,
        &pick_commit[..7]
    );

    Ok(())
}

/// Cancels an in-progress cherry-pick, restoring the working tree and index to
/// exactly match HEAD (which never moved during a conflicted pick) and discarding
/// any partially-applied changes or conflict-marker files it left behind.
// Used by `cherry-pick --abort`.
fn cherry_pick_abort() -> Result<()> {
    if !Path::new(".git/CHERRY_PICK_HEAD").exists() {
        anyhow::bail!("fatal: no cherry-pick in progress");
    }

    let pick_commit = fs::read_to_string(".git/CHERRY_PICK_HEAD")?.trim().to_string();
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
    let our_tree = resolve_tree_from_source(&our_commit)?;

    let (object_type, content) = read_object(&pick_commit)?;
    let (merged_tree, conflicts) = if object_type == "commit" {
        let text = String::from_utf8_lossy(&content).to_string();
        let parents = commit_parents(&text);
        let base_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
            resolve_tree_from_source(parent)?
        } else {
            BTreeMap::new()
        };
        let their_tree = resolve_tree_from_source(&pick_commit)?;
        three_way_tree_merge(&base_tree, &our_tree, &their_tree)?
    } else {
        (BTreeMap::new(), Vec::new())
    };

    let index_entries = read_index().unwrap_or_default();
    let mut tracked_paths: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();
    tracked_paths.extend(our_tree.keys().cloned());
    tracked_paths.extend(merged_tree.keys().cloned());
    tracked_paths.extend(conflicts.iter().map(|c| c.path.clone()));

    hard_reset_working_tree(&our_tree, &tracked_paths)?;

    let mut new_index_entries = build_index_entries_for_tree(&our_tree, true)?;
    write_index(&mut new_index_entries)?;

    let _ = fs::remove_file(".git/CHERRY_PICK_HEAD");
    let _ = fs::remove_file(".git/CHERRY_PICK_MSG");

    println!("Cherry-pick of {} aborted; HEAD left unchanged.", &pick_commit[..7]);

    Ok(())
}