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

    for file_path in files_to_add {
        let content = fs::read(&file_path)
            .with_context(|| format!("Failed to read {}", file_path.display()))?;

        let hash_hex = write_object("blob", &content)?;
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&hex::decode(&hash_hex)?);

        let metadata = fs::metadata(&file_path)?;
        let rel_path = normalize_path(&file_path);
        let new_entry = build_entry(&rel_path, hash, &metadata);

        entries.retain(|e| e.path != rel_path);
        entries.push(new_entry);
    }

    write_index(&mut entries)?;
    println!("Staged files to index.");

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
