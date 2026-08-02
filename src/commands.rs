use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::helpers::*;

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
    let ref_path = current_branch_ref()?;
    let ref_file = format!(".git/{}", ref_path);

    let parent_hash = if Path::new(&ref_file).exists() {
        Some(fs::read_to_string(&ref_file)?.trim().to_string())
    } else {
        None
    };

    if let Some(parent) = &parent_hash {
        if tree_hash_of_commit(parent)? == tree_hash {
            anyhow::bail!("nothing to commit (working tree matches HEAD)");
        }
    }

    let commit_hash = build_commit(tree_hash, parent_hash.clone(), &message)?;

    if let Some(parent_dir) = Path::new(&ref_file).parent() {
        fs::create_dir_all(parent_dir)?;
    }
    fs::write(&ref_file, format!("{}\n", commit_hash))?;

    let branch_name = ref_path.strip_prefix("refs/heads/").unwrap_or(&ref_path);
    let short_hash = &commit_hash[..7];

    if parent_hash.is_none() {
        println!("[{} (root-commit) {}] {}", branch_name, short_hash, message);
    } else {
        println!("[{} {}] {}", branch_name, short_hash, message);
    }

    Ok(())
}