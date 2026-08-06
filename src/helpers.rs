use std::collections::BTreeMap;
use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use crate::index::{read_index, write_index, build_entry};

pub fn write_object(object_type: &str, content: &[u8]) -> Result<String> {
    let header = format!("{} {}\0", object_type, content.len());

    let mut store = header.into_bytes();
    store.extend_from_slice(content);

    let mut hasher = Sha1::new();
    hasher.update(&store);

    let hash = hasher.finalize();
    let hash_hex = hex::encode(hash);

    let dir = format!(".git/objects/{}", &hash_hex[..2]);
    let path = format!("{}/{}", dir, &hash_hex[2..]);

    fs::create_dir_all(&dir)?;

    if !Path::new(&path).exists() {
        let file = fs::File::create(&path)?;
        let mut encoder = ZlibEncoder::new(file, Compression::default());

        encoder.write_all(&store)?;
        encoder.finish()?;
    }

    Ok(hash_hex)
}

pub fn read_object(hash: &str) -> Result<(String, Vec<u8>)> {
    if hash.len() < 3 {
        anyhow::bail!("Invalid object hash: {}", hash);
    }

    let dir = &hash[0..2];
    let file_name = &hash[2..];
    let path = format!(".git/objects/{}/{}", dir, file_name);
    let compressed_data = fs::read(&path).context("Failed to read object file (does this hash exist?)")?;
    let mut decoder = ZlibDecoder::new(&compressed_data[..]);
    let mut decompressed_data = Vec::new();
    decoder.read_to_end(&mut decompressed_data)?;

    let null_pos = decompressed_data
        .iter()
        .position(|&b| b == 0)
        .context("Invalid Git object format")?;

    let header = String::from_utf8_lossy(&decompressed_data[..null_pos]);
    let object_type = header
        .split(' ')
        .next()
        .context("Invalid Git object header")?
        .to_string();

    let content = decompressed_data[null_pos + 1..].to_vec();

    Ok((object_type, content))
}



pub fn tree_hash_of_commit(commit_hash: &str) -> Result<String> {
    let (object_type, content) = read_object(commit_hash)?;

    if object_type != "commit" {
        anyhow::bail!("{} is not a commit object", commit_hash);
    }

    let text = String::from_utf8_lossy(&content);
    let tree_line = text
        .lines()
        .next()
        .context("Malformed commit: missing tree line")?;

    tree_line
        .strip_prefix("tree ")
        .map(|s| s.to_string())
        .context("Malformed commit: first line isn't a tree line")
}

pub fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("path '{}' did not match any files", path.display()))?;

    if metadata.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }

    if metadata.is_dir() {
        let mut dir_entries: Vec<_> = fs::read_dir(path)?.filter_map(Result::ok).collect();
        dir_entries.sort_by_key(|e| e.file_name());

        for entry in dir_entries {
            let name_str = entry.file_name().to_string_lossy().to_string();

            if name_str == ".git" || name_str == "target" || name_str == ".idea" || name_str.starts_with('.') {
                continue;
            }

            collect_files(&entry.path(), out)?;
        }
    }

    Ok(())
}

pub fn normalize_path(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

pub fn hash_content(obj_type: &str, content: &[u8]) -> [u8; 20] {
    let header = format!("{} {}\0", obj_type, content.len());
    let mut store = header.into_bytes();
    store.extend_from_slice(content);
    let mut hasher = Sha1::new();
    hasher.update(&store);
    hasher.finalize().into()
}

pub fn flatten_tree(tree_hash: &str, prefix: &str, out: &mut BTreeMap<String, ([u8; 20], u32)>) -> Result<()> {
    let (object_type, content) = read_object(tree_hash)?;
    if object_type != "tree" {
        anyhow::bail!("{} is not a tree object", tree_hash);
    }

    let mut pos = 0;
    while pos < content.len() {
        let space_pos = content[pos..].iter().position(|&b| b == b' ').context("Missing mode separator")? + pos;
        let mode_str = String::from_utf8_lossy(&content[pos..space_pos]).to_string();

        let null_pos = content[space_pos..].iter().position(|&b| b == 0).context("Missing name terminator")? + space_pos;
        let name = String::from_utf8_lossy(&content[space_pos + 1..null_pos]).to_string();

        let hash_start = null_pos + 1;
        let hash_end = hash_start + 20;
        let mut hash = [0u8; 20];
        hash.copy_from_slice(&content[hash_start..hash_end]);

        let full_path = if prefix.is_empty() { name } else { format!("{}/{}", prefix, name) };

        if mode_str == "40000" {
            flatten_tree(&hex::encode(hash), &full_path, out)?;
        } else {
            let mode = u32::from_str_radix(&mode_str, 8).unwrap_or(0o100644);
            out.insert(full_path, (hash, mode));
        }

        pos = hash_end;
    }

    Ok(())
}

pub fn check_switch_safety(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>, index_map: &BTreeMap<String, [u8; 20]>) -> Result<()> {
    let mut working_files = Vec::new();
    collect_files(Path::new("."), &mut working_files)?;
    let mut working_map = BTreeMap::new();
    for file_path in &working_files {
        let rel_path = normalize_path(file_path);
        let content = fs::read(file_path)?;
        working_map.insert(rel_path, hash_content("blob", &content));
    }

    let mut local_changes = std::collections::BTreeSet::new();

    for (path, work_hash) in &working_map {
        match index_map.get(path) {
            None => {
                if let Some((target_hash, _)) = target_tree.get(path) {
                    if target_hash != work_hash {
                        anyhow::bail!("error: The following untracked working tree files would be overwritten by switch:\n\t{}\nPlease move or remove them before you switch branches.", path);
                    }
                }
            }
            Some(idx_hash) => {
                if idx_hash != work_hash {
                    local_changes.insert(path.clone());
                }
            }
        }
    }
    for path in index_map.keys() {
        if !working_map.contains_key(path) {
            local_changes.insert(path.clone());
        }
    }

    for (path, idx_hash) in index_map {
        match head_tree.get(path) {
            None => {
                local_changes.insert(path.clone());
            }
            Some((head_hash, _)) => {
                if head_hash != idx_hash {
                    local_changes.insert(path.clone());
                }
            }
        }
    }
    for path in head_tree.keys() {
        if !index_map.contains_key(path) {
            local_changes.insert(path.clone());
        }
    }

    let mut overwritten_files = Vec::new();
    for path in &local_changes {
        // A local change only conflicts if the switch would actually touch this file.
        // If HEAD and target have the same blob, sync_working_tree will skip it,
        // so the local edit carries over safely — no conflict.
        let file_changes_in_switch = match (head_tree.get(path), target_tree.get(path)) {
            (Some((h, _)), Some((t, _))) => h != t, // file exists in both but differs
            (None, Some(_)) => true,                 // new file introduced by target
            (Some(_), None) => true,                 // file deleted by target
            (None, None) => false,                   // file doesn't exist in either
        };

        if !file_changes_in_switch {
            continue; // switch won't touch this file — local edit is safe to carry
        }

        let current_hash_in_work_or_index = working_map.get(path)
            .or_else(|| index_map.get(path));

        let target_hash = target_tree.get(path).map(|(h, _)| h);

        if let Some(curr_hash) = current_hash_in_work_or_index {
            if let Some(t_hash) = target_hash {
                if curr_hash != t_hash {
                    overwritten_files.push(path.clone());
                }
            } else {
                overwritten_files.push(path.clone());
            }
        } else {
            if target_hash.is_some() {
                overwritten_files.push(path.clone());
            }
        }
    }

    if !overwritten_files.is_empty() {
        let mut msg = String::from("error: Your local changes to the following files would be overwritten by switch:\n");
        for file in overwritten_files {
            msg.push_str(&format!("\t{}\n", file));
        }
        msg.push_str("Please commit your changes or stash them before you switch branches.\nAborting");
        anyhow::bail!(msg);
    }

    Ok(())
}

pub fn sync_working_tree(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>, index_map: &BTreeMap<String, [u8; 20]>) -> Result<()> {
    let mut files_to_delete = std::collections::BTreeSet::new();
    for path in index_map.keys() {
        if !target_tree.contains_key(path) {
            files_to_delete.insert(path.clone());
        }
    }
    for path in head_tree.keys() {
        if !target_tree.contains_key(path) {
            files_to_delete.insert(path.clone());
        }
    }

    for path in files_to_delete {
        let path_obj = Path::new(&path);
        if path_obj.exists() {
            fs::remove_file(path_obj)?;
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

    for (path, (hash, _mode)) in target_tree {
        let should_write = match head_tree.get(path) {
            None => true,                          
            Some((head_hash, _)) => head_hash != hash, 
        };

        if should_write {
            let (_, content) = read_object(&hex::encode(hash))?;
            if let Some(parent) = Path::new(&path).parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, &content)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let file_mode = if (_mode & 0o111) != 0 { 0o755 } else { 0o644 };
                fs::set_permissions(path, fs::Permissions::from_mode(file_mode))?;
            }
        }
    }

    Ok(())
}

pub fn update_index_from_tree(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>) -> Result<()> {
    let mut entries = read_index().unwrap_or_default();
    entries.retain(|e| target_tree.contains_key(&e.path));

    for (path, (hash, _)) in target_tree {
        let should_update = match head_tree.get(path) {
            None => true,
            Some((head_hash, _)) => head_hash != hash,
        };

        if should_update {
            let metadata = fs::metadata(path)?;
            let new_entry = build_entry(path, *hash, &metadata);
            entries.retain(|e| &e.path != path);
            entries.push(new_entry);
        }
    }

    write_index(&mut entries)?;
    Ok(())
}