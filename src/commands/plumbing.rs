use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use anyhow::Context;
use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};
use crate::helpers::{read_object, write_object};
use crate::index::{read_index, IndexEntry};

pub fn init() -> anyhow::Result<()> {
    fs::create_dir_all(".git/objects")?;
    fs::create_dir_all(".git/refs/heads")?;
    fs::write(".git/HEAD", "ref: refs/heads/main\n")?;
    println!("Initialized empty git repository in .git/");
    Ok(())
}

pub fn hash_object(write: bool, file: PathBuf) -> anyhow::Result<()> {
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

pub fn cat_file(pretty_print: bool, object_hash: String) -> anyhow::Result<()> {
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

pub fn write_tree() -> anyhow::Result<()> {
    let entries = read_index().unwrap_or_default();
    let tree_hash = write_tree_from_index_prefix(&entries, "")?;
    println!("{}", tree_hash);
    Ok(())
}

pub(crate) fn write_tree_from_index_prefix(entries: &[IndexEntry], prefix: &str) -> anyhow::Result<String> {
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

pub fn ls_tree(name_only: bool, tree_hash: String) -> anyhow::Result<()> {
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