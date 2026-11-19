use std::io::{self, Read, Write};
use anyhow::Result;

use crate::core::pack::parse_pack_index;

/// `rgit show-index`
///
/// Reads a pack index (`.idx`) from standard input and lists it: one line per object, in
/// index order (sorted by object name), `<offset in the pack> <object name> (<crc32>)`; the
/// CRC is missing for old version 1 indexes. Works outside a repository. Exit status: 0,
/// 128 when the input is not a valid index, 129 for a usage error.
pub fn show_index(args: Vec<String>) -> Result<()> {
    if let Some(arg) = args.first() {
        if arg == "-h" {
            println!("usage: rgit show-index");
        } else {
            eprintln!("usage: rgit show-index");
        }
        crate::ui::exit(129);
    }
    let mut data = Vec::new();
    io::stdin().lock().read_to_end(&mut data)?;
    let index = match parse_pack_index(&data) {
        Ok(index) => index,
        Err(e) => {
            eprintln!("{}", e);
            crate::ui::exit(128);
        }
    };
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for (i, hash) in index.hashes.iter().enumerate() {
        match index.crcs.get(i) {
            Some(crc) => writeln!(out, "{} {} ({:08x})", index.offsets[i], hex::encode(hash), crc)?,
            None => writeln!(out, "{} {}", index.offsets[i], hex::encode(hash))?,
        }
    }
    out.flush()?;
    Ok(())
}
