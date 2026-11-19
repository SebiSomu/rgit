use std::collections::BTreeMap;
use std::fs;
use anyhow::Result;

use crate::core::pack::{index_pack_bytes, pack_and_index_paths, parse_pack_index};

/// `rgit verify-pack [-v | --verbose] [-s | --stat-only] <pack>.idx...`
///
/// Checks packfiles: the pack's checksum, every entry (inflating it and resolving its delta
/// chain), and that the index lists exactly these objects at these offsets with these
/// CRCs. Each argument may be the `.idx`, the `.pack` or their common base name. `-v` lists
/// every object in pack order — `<name> <type> <size> <size in pack> <offset>`, plus
/// `<depth> <base name>` for deltas (whose size is that of the delta) — then a histogram of
/// delta chain lengths and `<pack>: ok`; `-s` prints only the histogram. Exit status: 0,
/// 1 when a pack is damaged or cannot be read, 129 for a usage error.
pub fn verify_pack(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit verify-pack [-v | --verbose] [-s | --stat-only] [--] <pack>.idx...\n\n    -v, --[no-]verbose    verbose\n    -s, --[no-]stat-only  show statistics only\n\n";
    let (mut verbose, mut stat_only) = (false, false);
    let mut packs: Vec<String> = Vec::new();
    let mut options_done = false;
    for arg in &args {
        match arg.as_str() {
            _ if options_done => packs.push(arg.clone()),
            "--" => options_done = true,
            "-v" | "--verbose" => verbose = true,
            "--no-verbose" => verbose = false,
            "-s" | "--stat-only" => stat_only = true,
            "--no-stat-only" => stat_only = false,
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            other if other.starts_with('-') && other.len() > 1 => {
                let shown = if let Some(long) = other.strip_prefix("--") { format!("unknown option `{}'", long) } else { format!("unknown switch `{}'", &other[1..2]) };
                eprint!("error: {}\n{}", shown, USAGE);
                crate::ui::exit(129);
            }
            _ => packs.push(arg.clone()),
        }
    }
    if packs.is_empty() {
        eprint!("{}", USAGE);
        crate::ui::exit(129);
    }

    let mut failed = false;
    for arg in &packs {
        let (pack_path, index_path) = pack_and_index_paths(arg);
        let shown = pack_path.display().to_string();
        let checked = (|| -> Result<Vec<crate::objects::PackedObject>> {
            let pack = fs::read(&pack_path).map_err(|_| anyhow::anyhow!("fatal: Cannot open existing pack file '{}'", arg))?;
            let stored = fs::read(&index_path).map_err(|_| anyhow::anyhow!("fatal: Cannot open existing pack idx file for '{}'", arg))?;
            let stored = parse_pack_index(&stored)?;
            let (_, _, objects) = index_pack_bytes(pack.clone(), false)?;
            if stored.pack_checksum != pack[pack.len() - 20..] {
                anyhow::bail!("error: {}: index does not belong to this pack (checksum mismatch)", shown);
            }
            if stored.hashes.len() != objects.len() {
                anyhow::bail!("error: {}: index lists {} objects but the pack holds {}", shown, stored.hashes.len(), objects.len());
            }
            for object in &objects {
                let mut hash = [0u8; 20];
                hex::decode_to_slice(&object.hash, &mut hash)?;
                match stored.hashes.binary_search(&hash) {
                    Ok(i) if stored.offsets[i] == object.offset && stored.crcs.get(i).is_none_or(|crc| *crc == object.crc) => {}
                    Ok(_) => anyhow::bail!("error: {}: index entry for {} does not match the pack", shown, object.hash),
                    Err(_) => anyhow::bail!("error: {}: object {} is missing from the index", shown, object.hash),
                }
            }
            Ok(objects)
        })();
        let objects = match checked {
            Ok(objects) => objects,
            Err(e) => {
                eprintln!("{}", e);
                failed = true;
                continue;
            }
        };
        if !(verbose || stat_only) {
            continue;
        }
        let mut chains: BTreeMap<u32, usize> = BTreeMap::new();
        for object in &objects {
            *chains.entry(object.depth).or_default() += 1;
            if stat_only {
                continue;
            }
            print!("{} {:<6} {} {} {}", object.hash, object.kind, object.header_size, object.end - object.offset, object.offset);
            match &object.base {
                Some(base) => println!(" {} {}", object.depth, base),
                None => println!(),
            }
        }
        let plural = |n: usize| if n == 1 { "object" } else { "objects" };
        for (depth, count) in &chains {
            if *depth == 0 {
                println!("non delta: {} {}", count, plural(*count));
            } else {
                println!("chain length = {}: {} {}", depth, count, plural(*count));
            }
        }
        if !stat_only {
            println!("{}: ok", shown);
        }
    }
    if failed {
        crate::ui::exit(1);
    }
    Ok(())
}
