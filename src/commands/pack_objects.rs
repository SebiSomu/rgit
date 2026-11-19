use std::collections::HashSet;
use std::io::{self, BufRead, Write};
use std::path::Path;
use anyhow::Result;

use crate::helpers::pack::{build_pack, packed_object_exists, reachable_objects, store_pack, write_pack_index};
use crate::helpers::rev_list::list_all_refs;
use crate::helpers::rev_parse::resolve_rev;
use crate::objects::PackBuildOptions;
use crate::repo;

/// `rgit pack-objects [--revs] [--all] [--unpacked | --incremental] [--non-empty] [-q]
/// [--progress] [--window=<n>] [--depth=<n>] (--stdout | <base-name>)`
///
/// Creates a packfile. The objects to pack are read from standard input, one name per line
/// (anything after the name, such as the path `rev-list --objects` prints, is ignored).
/// With `--revs` the input is revisions instead — `<rev>`, `^<rev>` or `--not` — and
/// everything reachable from the positive ones but not from the negative ones is packed;
/// `--all` adds every ref. `--unpacked` / `--incremental` leave out objects that are already
/// in a pack. The pack goes to standard output with `--stdout`, or to
/// `<base-name>-<checksum>.pack` with its `.idx`, the checksum being printed. Trees and
/// blobs are stored as deltas against one of the previous `--window` objects of their kind
/// (default 10; 0 disables deltas) when that saves at least half, in chains of at most
/// `--depth` (default 50). `--non-empty` writes nothing when there is nothing to pack;
/// `--progress` prints the totals to standard error. Exit status: 0, 128 on errors, 129
/// for a usage error.
pub fn pack_objects(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit pack-objects [-q | --progress] [--revs] [--all] [--unpacked | --incremental] [--non-empty]\n                         [--window=<n>] [--depth=<n>] (--stdout | <base-name>) < <object-list>\n";
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        crate::ui::exit(code);
    };
    let (mut revs, mut all, mut stdout, mut unpacked, mut non_empty, mut progress) = (false, false, false, false, false, false);
    let mut build = PackBuildOptions { window: 10, depth: 50 };
    let mut base_name: Option<String> = None;
    for arg in &args {
        let number = |value: &str| -> usize { value.parse().unwrap_or_else(|_| die(USAGE, 129)) };
        match arg.as_str() {
            "--revs" => revs = true,
            "--all" => {
                all = true;
                revs = true;
            }
            "--stdout" => stdout = true,
            "--unpacked" | "--incremental" => unpacked = true,
            "--non-empty" => non_empty = true,
            "--progress" | "--all-progress" => progress = true,
            "-q" => progress = false,
            "--all-progress-implied" | "--no-reuse-delta" | "--no-reuse-object" | "--delta-base-offset" | "--include-tag" | "--honor-pack-keep" | "--keep-true-parents" | "--local" | "--thin" | "--reflog" | "--indexed-objects" | "--keep-unreachable" | "--pack-loose-unreachable" => {}
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            other if other.starts_with("--window=") => build.window = number(&other["--window=".len()..]),
            other if other.starts_with("--depth=") => build.depth = number(&other["--depth=".len()..]) as u32,
            other if other.starts_with("--threads=") || other.starts_with("--compression=") || other.starts_with("--keep-pack=") => {}
            other if other.starts_with('-') => die(&format!("error: unknown option `{}'\n{}", other.trim_start_matches('-'), USAGE), 129),
            other => {
                if base_name.is_some() {
                    die(USAGE, 129);
                }
                base_name = Some(other.to_string());
            }
        }
    }
    if stdout == base_name.is_some() {
        die(USAGE, 129);
    }
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git\n", 128);
    }

    let mut objects: Vec<String> = Vec::new();
    if revs {
        let (mut wanted, mut unwanted): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        if all {
            wanted.extend(list_all_refs().into_values());
            if let Some(head) = crate::refs::resolve_head_commit()? {
                wanted.push(head);
            }
        }
        let mut negate = false;
        for line in io::stdin().lock().lines() {
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "--not" {
                negate = !negate;
                continue;
            }
            let (name, negative) = match line.strip_prefix('^') {
                Some(rest) => (rest, !negate),
                None => (line, negate),
            };
            let Ok(hash) = resolve_rev(name) else { die(&format!("fatal: bad revision '{}'\n", name), 128) };
            if negative { unwanted.push(hash) } else { wanted.push(hash) }
        }
        let excluded: HashSet<String> = reachable_objects(&unwanted, &HashSet::new()).into_iter().collect();
        objects = reachable_objects(&wanted, &excluded);
    } else {
        let mut seen = HashSet::new();
        for line in io::stdin().lock().lines() {
            let line = line?;
            if line.is_empty() {
                continue;
            }
            let name = line.split(' ').next().unwrap_or("");
            if name.len() != 40 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
                die(&format!("fatal: expected object ID, got garbage:\n {}\n", line), 128);
            }
            let name = name.to_ascii_lowercase();
            if seen.insert(name.clone()) {
                objects.push(name);
            }
        }
    }
    if unpacked {
        objects.retain(|name| !packed_object_exists(name));
    }
    if objects.is_empty() && non_empty {
        return Ok(());
    }

    let built = match build_pack(&objects, &build) {
        Ok(built) => built,
        Err(e) => die(&format!("{}\n", e), 128),
    };
    if progress {
        eprintln!("Total {} (delta {}), reused 0 (delta 0), pack-reused 0", objects.len(), built.deltas);
    }
    if stdout {
        io::stdout().lock().write_all(&built.bytes)?;
        return Ok(());
    }
    let mut checksum = [0u8; 20];
    checksum.copy_from_slice(&built.bytes[built.bytes.len() - 20..]);
    let index = write_pack_index(&built.entries, &checksum);
    let base = base_name.unwrap_or_default();
    let path = Path::new(&base);
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let prefix = format!("{}-", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    println!("{}", store_pack(dir, &prefix, &built.bytes, &index)?);
    Ok(())
}
