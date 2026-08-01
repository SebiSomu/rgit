use anyhow::{Context, Result};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use std::fs;
use std::io::{Write};
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