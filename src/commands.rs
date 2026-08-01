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