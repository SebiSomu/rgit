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
    let tree_hash = write_tree_recursive(Path::new("."))?;
    println!("{}", tree_hash);
    Ok(())
}

fn write_tree_recursive(dir_path: &Path) -> Result<String> {
    let mut paths: Vec<_> = fs::read_dir(dir_path)?.filter_map(Result::ok).collect();
    paths.sort_by_key(|entry| entry.file_name());

    let mut tree_content = Vec::new();

    for entry in paths {
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => continue,
        };

        if name == ".git" || name == "target" || name == ".idea" || name.starts_with('.') {
            continue;
        }

        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            let tree_hash = write_tree_recursive(&entry.path())?;
            let tree_hash_bytes = hex::decode(tree_hash)?;

            tree_content.extend_from_slice(b"40000 ");
            tree_content.extend_from_slice(name.as_bytes());
            tree_content.push(0);
            tree_content.extend_from_slice(&tree_hash_bytes);
        } else if metadata.is_file() {
            let content = fs::read(entry.path())?;
            let blob_hash = write_object("blob", &content)?;
            let blob_hash_bytes = hex::decode(blob_hash)?;

            tree_content.extend_from_slice(b"100644 ");
            tree_content.extend_from_slice(name.as_bytes());
            tree_content.push(0);
            tree_content.extend_from_slice(&blob_hash_bytes);
        }
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
    let tree_hash = write_tree_recursive(Path::new("."))?;
    let ref_path = refs::current_branch_ref()?;
    let parent_hash = refs::read_ref(&ref_path)?;

    if let Some(parent) = &parent_hash {
        if tree_hash_of_commit(parent)? == tree_hash {
            anyhow::bail!("nothing to commit (working tree matches HEAD)");
        }
    }

    let commit_hash = build_commit(tree_hash, parent_hash.clone(), &message)?;
    refs::write_ref(&ref_path, &commit_hash)?;

    let branch_name = ref_path.strip_prefix("refs/heads/").unwrap_or(&ref_path);
    let short_hash = &commit_hash[..7];

    if parent_hash.is_none() {
        println!("[{} (root-commit) {}] {}", branch_name, short_hash, message);
    } else {
        println!("[{} {}] {}", branch_name, short_hash, message);
    }

    Ok(())
}

pub fn log(oneline: bool) -> Result<()> {
    let ref_path = refs::current_branch_ref()?;
    let branch_name = ref_path.strip_prefix("refs/heads/").unwrap_or(&ref_path);

    let mut current_hash = refs::read_ref(&ref_path)?;

    if current_hash.is_none() {
        anyhow::bail!(
            "fatal: your current branch '{}' does not have any commits yet",
            branch_name
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
    let ref_path = refs::current_branch_ref()?;
    let branch_name = ref_path.strip_prefix("refs/heads/").unwrap_or(&ref_path).to_string();

    println!("On branch {}", branch_name);

    let mut no_commits_yet = false;
    let mut head_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();

    if let Some(commit_hash) = refs::read_ref(&ref_path)? {
        let tree_hash = tree_hash_of_commit(&commit_hash)?;
        flatten_tree(&tree_hash, "", &mut head_tree)?;
    } else {
        no_commits_yet = true;
        println!("\nNo commits yet");
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

pub fn branch(name: Option<String>) -> Result<()> {
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