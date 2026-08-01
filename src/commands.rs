use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

pub fn init() -> Result<()> {
    fs::create_dir_all(".git/objects")?;
    fs::create_dir_all(".git/refs/heads")?;

    fs::write(".git/HEAD", "ref: refs/heads/main\n")?;

    println!("Initialized empty git repository in .git/");

    Ok(())
}

pub fn hash_object(write: bool, file: PathBuf) -> Result<()> {
    let content = fs::read(&file).context("Failed to read file")?;
    let header = format!("blob {}\0", content.len());
    let mut store = header.into_bytes();
    store.extend_from_slice(&content);

    let mut hasher = Sha1::new();
    hasher.update(&store);
    let hash_hex = hex::encode(hasher.finalize());

    if write {
        let dir = format!(".git/objects/{}", &hash_hex[0..2]);
        let path = format!("{}/{}", dir, &hash_hex[2..]);

        fs::create_dir_all(&dir)?;

        let file = fs::File::create(&path)?;
        let mut encoder = ZlibEncoder::new(file, Compression::default());
        encoder.write_all(&store)?;
    }

    println!("{}", hash_hex);
    Ok(())
}

pub fn cat_file(pretty_print: bool, object_hash: String) -> Result<()> {
    let dir = &object_hash[0..2];
    let file_name = &object_hash[2..];
    let path = format!(".git/objects/{}/{}", dir, file_name);

    let compressed_data = fs::read(&path).context("Failed to read object file (does this hash exist?)")?;
    let mut decoder = ZlibDecoder::new(&compressed_data[..]);
    let mut decompressed_data = Vec::new();
    decoder.read_to_end(&mut decompressed_data)?;

    let null_pos = decompressed_data.iter().position(|&b| b == 0).context("Invalid Git object format")?;

    if pretty_print {
        let content = &decompressed_data[null_pos + 1..];
        let text = String::from_utf8_lossy(content);
        println!("{}", text);
    }

    Ok(())
}

pub fn write_tree() -> Result<()> {
    let tree_hash = write_tree_recursive(std::path::Path::new("."))?;
    println!("{}", tree_hash);
    Ok(())
}

fn write_tree_recursive(dir_path: &std::path::Path) -> Result<String> {
    let mut paths: Vec<_> = fs::read_dir(dir_path)?
        .filter_map(Result::ok)
        .collect();

    paths.sort_by_key(|dir| dir.file_name());

    let mut tree_content = Vec::new();

    for entry in paths {
        let file_name_os = entry.file_name();
        let name = match file_name_os.into_string() {
            Ok(n) => n,
            Err(_) => return Ok("".to_string()),
        };

        if name == ".git" || name == "target" || name == ".idea" || name.starts_with('.') {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if metadata.is_dir() {
            let tree_hash_hex = write_tree_recursive(&entry.path())?;
            let tree_hash_bytes = hex::decode(tree_hash_hex)?;

            tree_content.extend_from_slice(b"40000 ");
            tree_content.extend_from_slice(name.as_bytes());
            tree_content.push(0);
            tree_content.extend_from_slice(&tree_hash_bytes);
        } else if metadata.is_file() {
            let content = match fs::read(entry.path()) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let header = format!("blob {}\0", content.len());
            let mut store = header.into_bytes();
            store.extend_from_slice(&content);

            let mut hasher = Sha1::new();
            hasher.update(&store);
            let hash_bytes = hasher.finalize();
            let hash_hex = hex::encode(&hash_bytes);

            let blob_dir = format!(".git/objects/{}", &hash_hex[0..2]);
            let blob_path = format!("{}/{}", blob_dir, &hash_hex[2..]);
            fs::create_dir_all(&blob_dir)?;

            if !std::path::Path::new(&blob_path).exists() {
                let file = fs::File::create(&blob_path)?;
                let mut encoder = ZlibEncoder::new(file, Compression::default());
                encoder.write_all(&store)?;
                encoder.finish()?;
            }

            tree_content.extend_from_slice(b"100644 ");
            tree_content.extend_from_slice(name.as_bytes());
            tree_content.push(0);
            tree_content.extend_from_slice(&hash_bytes);
        }
    }

    let header = format!("tree {}\0", tree_content.len());
    let mut store = header.into_bytes();
    store.extend_from_slice(&tree_content);

    let mut hasher = Sha1::new();
    hasher.update(&store);
    let hash_bytes = hasher.finalize();
    let hash_hex = hex::encode(&hash_bytes);

    let tree_dir = format!(".git/objects/{}", &hash_hex[0..2]);
    let tree_path = format!("{}/{}", tree_dir, &hash_hex[2..]);
    fs::create_dir_all(&tree_dir)?;
    let file = fs::File::create(&tree_path)
        .with_context(|| format!("creating tree object {}", tree_path))?;
    let mut encoder = ZlibEncoder::new(file, Compression::default());
    encoder.write_all(&store)?;

    Ok(hash_hex)
}