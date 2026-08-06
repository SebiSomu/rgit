use std::collections::BTreeMap;
use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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