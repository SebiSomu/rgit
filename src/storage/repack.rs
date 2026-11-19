use std::collections::HashSet;
use std::fs;
use anyhow::Result;

use crate::core::object_store::list_loose_objects;
use crate::storage::gc::engine::{collect_lenient_gc_roots, collect_strict_gc_roots};
use crate::core::object_store::write_object;
use crate::core::pack::{build_pack, forget_repo_packs, packed_object_exists, prune_packed_objects, reachable_objects, read_packed_object, repo_packs, store_pack, write_pack_index};
use crate::core::types::PackBuildOptions;
use crate::core::repo;

/// `rgit repack [-a | -A] [-d] [-q] [-f] [-F] [-l] [-n] [--window=<n>] [--depth=<n>]`
///
/// Packs objects into a packfile under `.git/objects/pack`. Without `-a`, the loose objects
/// that are not in a pack yet go into a new pack. With `-a`, everything reachable from the
/// refs, their reflogs, `HEAD` and the index goes into one new pack. `-d` then removes what
/// became redundant: the loose copies of packed objects, and with `-a` the older packs
/// (packs with a `.keep` file stay). Unreachable objects that only lived in those older
/// packs are dropped with `-a -d`; `-A -d` writes them back as loose objects instead, so
/// that `prune` can expire them by age. `--window` / `--depth` tune delta compression as in
/// `pack-objects`. Prints `Nothing new to pack.` when there is nothing to do. Exit status:
/// 0, 128 on errors, 129 for a usage error.
pub fn repack(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit repack [-a] [-A] [-d] [-f] [-F] [-l] [-n] [-q] [--window=<n>] [--depth=<n>]\n";
    let (mut all, mut loosen, mut delete) = (false, false, false);
    let mut build = PackBuildOptions { window: 10, depth: 50 };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        let mut number = |inline: Option<&str>| -> usize {
            let value = match inline {
                Some(value) => value.to_string(),
                None => {
                    i += 1;
                    args.get(i - 1).cloned().unwrap_or_default()
                }
            };
            value.parse().unwrap_or_else(|_| {
                eprint!("{}", USAGE);
                crate::ui::exit(129);
            })
        };
        match arg {
            "-a" => all = true,
            "-A" => {
                all = true;
                loosen = true;
            }
            "-d" => delete = true,
            "-q" | "--quiet" | "-f" | "-F" | "-l" | "--local" | "-n" | "-k" | "--keep-unreachable" | "-b" | "--write-bitmap-index" | "--no-write-bitmap-index" | "--pack-kept-objects" | "-i" | "--delta-islands" => {}
            "--window" => build.window = number(None),
            "--depth" => build.depth = number(None) as u32,
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            other if other.starts_with("--window=") => build.window = number(Some(&other["--window=".len()..])),
            other if other.starts_with("--depth=") => build.depth = number(Some(&other["--depth=".len()..])) as u32,
            other if other.starts_with("--threads=") || other.starts_with("--unpack-unreachable=") || other.starts_with("--keep-pack=") || other.starts_with("--max-pack-size=") => {}
            other if other.len() > 2 && other.starts_with('-') && !other.starts_with("--") && other[1..].chars().all(|c| "aAdqfFln".contains(c)) => {
                // Combined short switches such as `-ad` or `-Adq`.
                all |= other.contains('a') || other.contains('A');
                loosen |= other.contains('A');
                delete |= other.contains('d');
            }
            other => {
                eprint!("error: unknown option `{}'\n{}", other.trim_start_matches('-'), USAGE);
                crate::ui::exit(129);
            }
        }
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }

    let pack_dir = repo::path(".git/objects/pack");
    let old_packs: Vec<(String, Vec<String>)> = repo_packs().iter().map(|p| (p.name.clone(), p.index.hashes.iter().map(hex::encode).collect())).collect();
    let objects: Vec<String> = if all {
        // Everything that keeps objects alive: refs (loose and packed), HEAD, the index and
        // in-progress state of every worktree, plus all reflog entries.
        let mut roots = collect_strict_gc_roots()?;
        roots.extend(collect_lenient_gc_roots(None)?);
        reachable_objects(&roots, &HashSet::new())
    } else {
        list_loose_objects()?.into_iter().map(|o| o.hash).filter(|hash| !packed_object_exists(hash)).collect()
    };
    if objects.is_empty() {
        println!("Nothing new to pack.");
        if delete {
            prune_packed_objects(false)?;
        }
        return Ok(());
    }

    let built = build_pack(&objects, &build)?;
    let mut checksum = [0u8; 20];
    checksum.copy_from_slice(&built.bytes[built.bytes.len() - 20..]);
    let index = write_pack_index(&built.entries, &checksum);
    let new_name = format!("pack-{}", store_pack(&pack_dir, "pack-", &built.bytes, &index)?);

    if delete {
        if all {
            let packed: HashSet<&String> = objects.iter().collect();
            for (name, hashes) in &old_packs {
                if *name == new_name || pack_dir.join(format!("{}.keep", name)).exists() {
                    continue;
                }
                if loosen {
                    // Keep unreachable objects around as loose objects.
                    for hash in hashes.iter().filter(|hash| !packed.contains(hash)) {
                        if let Some((kind, content)) = read_packed_object(hash) {
                            write_object(&kind, &content)?;
                        }
                    }
                }
                for extension in ["pack", "idx", "rev", "bitmap", "mtimes"] {
                    let path = pack_dir.join(format!("{}.{}", name, extension));
                    if path.exists() {
                        if let Ok(metadata) = fs::metadata(&path) {
                            let mut permissions = metadata.permissions();
                            #[allow(clippy::permissions_set_readonly_false)]
                            permissions.set_readonly(false);
                            let _ = fs::set_permissions(&path, permissions);
                        }
                        fs::remove_file(&path)?;
                    }
                }
            }
            // A multi-pack-index describing the removed packs is stale now.
            let _ = fs::remove_file(pack_dir.join("multi-pack-index"));
            forget_repo_packs();
        }
        prune_packed_objects(false)?;
    }
    Ok(())
}
