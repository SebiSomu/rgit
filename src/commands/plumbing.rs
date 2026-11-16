use std::fs;
use std::path::PathBuf;
use anyhow::Context;
use sha1::{Digest, Sha1};
use crate::helpers::{read_object, write_object, write_tree_from_index_prefix};
use crate::index::read_index;

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
    // An object with a `refs/replace/` entry shows its replacement, as in git.
    let object_hash = crate::helpers::objects::replacement_object(&object_hash).unwrap_or(object_hash);
    let (_, content) = crate::helpers::objects::read_object(&object_hash).context("Failed to read object file")?;

    if pretty_print {
        let text = String::from_utf8_lossy(&content);
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