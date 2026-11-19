use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;
use anyhow::Result;

use crate::helpers::pack::{index_pack_bytes, store_pack};
use crate::repo;

/// `rgit index-pack [-v] [-o <index-file>] [--verify] <pack-file>` /
/// `rgit index-pack --stdin [--fix-thin] [--keep[=<msg>]]`
///
/// Builds the index (`.idx`, version 2) for a packfile: reads every entry, resolves the
/// deltas to learn each object's name, and writes the sorted name table with offsets and
/// CRCs next to the pack (or to `-o <file>`), then prints the pack's checksum. With
/// `--stdin` the pack is read from standard input and stored as
/// `.git/objects/pack/pack-<checksum>.pack` with its index, printing `pack TAB <checksum>`
/// (`keep TAB ...` with `--keep`, which also writes a `.keep` file). `--fix-thin` (with
/// `--stdin`) accepts a thin pack — deltas against objects that are not in the pack but in
/// this repository — and completes it by appending those objects. `--verify` checks that
/// the existing index matches the pack instead of writing one. Exit status: 0, 128 on
/// errors (corrupt pack, unresolved deltas), 129 for a usage error.
pub fn index_pack(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit index-pack [-v] [-o <index-file>] [--keep | --keep=<msg>] [--verify] [--strict] (<pack-file> | --stdin [--fix-thin] [<pack-file>])\n";
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        crate::ui::exit(code);
    };
    let (mut stdin, mut fix_thin, mut verify) = (false, false, false);
    let mut keep: Option<String> = None;
    let mut index_file: Option<String> = None;
    let mut pack_file: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        match arg {
            "--stdin" => stdin = true,
            "--fix-thin" => fix_thin = true,
            "--verify" => verify = true,
            "--keep" => keep = Some(String::new()),
            "-v" | "--strict" | "--check-self-contained-and-connected" | "--rev-index" | "--no-rev-index" | "--show-stat" | "--verify-stat" | "--verify-stat-only" => {}
            "-o" => {
                index_file = Some(args.get(i).cloned().unwrap_or_else(|| die(USAGE, 129)));
                i += 1;
            }
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            other if other.starts_with("--keep=") => keep = Some(other["--keep=".len()..].to_string()),
            other if other.starts_with("--threads=") || other.starts_with("--strict=") || other.starts_with("--max-input-size=") || other.starts_with("--object-format=") => {}
            other if other.starts_with('-') => die(USAGE, 129),
            other => {
                if pack_file.is_some() {
                    die(USAGE, 129);
                }
                pack_file = Some(other.to_string());
            }
        }
    }
    if fix_thin && !stdin {
        die("fatal: --fix-thin cannot be used without --stdin\n", 128);
    }
    if stdin && !repo::exists() && pack_file.is_none() {
        die("fatal: --stdin requires a git repository\n", 128);
    }
    if !stdin && pack_file.is_none() {
        die(USAGE, 129);
    }
    if let Some(name) = pack_file.as_deref().filter(|name| !name.ends_with(".pack")) {
        die(&format!("fatal: packfile name '{}' does not end with '.pack'\n", name), 128);
    }

    let mut pack = Vec::new();
    if stdin {
        io::stdin().lock().read_to_end(&mut pack)?;
    } else {
        let name = pack_file.clone().unwrap_or_default();
        pack = fs::read(&name).unwrap_or_else(|e| die(&format!("fatal: cannot open packfile '{}': {}\n", name, e), 128));
    }
    let (pack, index, _) = match index_pack_bytes(pack, fix_thin) {
        Ok(result) => result,
        Err(e) => die(&format!("{}\n", e), 128),
    };
    let checksum = hex::encode(&pack[pack.len() - 20..]);

    if verify {
        let index_path = index_file.unwrap_or_else(|| format!("{}.idx", pack_file.clone().unwrap_or_default().trim_end_matches(".pack")));
        match fs::read(&index_path) {
            Ok(existing) if existing == index => {}
            Ok(_) => die(&format!("fatal: index file '{}' does not match the pack\n", index_path), 128),
            Err(e) => die(&format!("fatal: cannot open index file '{}': {}\n", index_path, e), 128),
        }
        return Ok(());
    }

    let base: PathBuf = match (&pack_file, stdin) {
        (Some(name), true) => {
            fs::write(name, &pack)?;
            PathBuf::from(name.trim_end_matches(".pack"))
        }
        (Some(name), false) => PathBuf::from(name.trim_end_matches(".pack")),
        (None, _) => {
            let dir = repo::path(".git/objects/pack");
            store_pack(&dir, "pack-", &pack, &index)?;
            dir.join(format!("pack-{}", checksum))
        }
    };
    if pack_file.is_some() {
        let index_path = index_file.unwrap_or_else(|| format!("{}.idx", base.display()));
        fs::write(&index_path, &index).unwrap_or_else(|e| die(&format!("fatal: unable to write index file '{}': {}\n", index_path, e), 128));
    }
    match (&keep, stdin) {
        (Some(message), _) => {
            fs::write(format!("{}.keep", base.display()), if message.is_empty() { String::new() } else { format!("{}\n", message) })?;
            println!("keep\t{}", checksum);
        }
        (None, true) => println!("pack\t{}", checksum),
        (None, false) => println!("{}", checksum),
    }
    Ok(())
}
