pub mod cli;

use std::io::{self, Read};
use anyhow::Result;

use crate::core::object_store::write_object;
use crate::core::pack::{resolve_pack_entries, scan_pack};
use crate::core::repo;

/// `rgit unpack-objects [-n] [-q] [-r] [--strict]`
///
/// Reads a packfile from standard input and writes every object in it as a loose object
/// ("explodes" the pack). Deltas are resolved against objects in the pack or already in
/// the repository. Objects the repository already has are left alone. `-n` only checks the
/// pack without writing; `-q` and `--strict` are accepted; `-r` keeps going past objects
/// that cannot be written. Exit status: 0, 128 on a corrupt pack or unresolved deltas,
/// 129 for a usage error.
pub fn unpack_objects(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit unpack-objects [-n] [-q] [-r] [--strict]\n";
    let (mut dry_run, mut recover) = (false, false);
    for arg in &args {
        match arg.as_str() {
            "-n" => dry_run = true,
            "-r" => recover = true,
            "-q" | "--strict" => {}
            other if other.starts_with("--strict=") || other.starts_with("--max-input-size=") || other.starts_with("--pack_header=") => {}
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            _ => {
                eprint!("{}", USAGE);
                crate::ui::exit(129);
            }
        }
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    let mut pack = Vec::new();
    io::stdin().lock().read_to_end(&mut pack)?;
    let objects = match scan_pack(&pack).and_then(|(entries, _)| resolve_pack_entries(&pack, &entries, true)) {
        Ok((objects, _)) => objects,
        Err(e) => {
            eprintln!("{}", e);
            crate::ui::exit(128);
        }
    };
    if dry_run {
        return Ok(());
    }
    let mut failed = false;
    for object in &objects {
        if let Err(e) = write_object(&object.kind, &object.content) {
            eprintln!("error: unable to write object {}: {}", object.hash, e);
            failed = true;
            if !recover {
                crate::ui::exit(128);
            }
        }
    }
    if failed {
        crate::ui::exit(1);
    }
    Ok(())
}
