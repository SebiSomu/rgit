pub mod engine;

use std::fs;
use anyhow::Result;

use crate::storage::multi_pack_index::engine::{midx_path, parse_midx, store_midx, verify_midx, write_midx};
use crate::core::pack::{forget_repo_packs, repo_packs};
use crate::objects::RepoPack;
use crate::core::repo;

/// `rgit multi-pack-index write [--preferred-pack=<pack>]` / `verify` / `expire`
///
/// Maintains the multi-pack-index (`.git/objects/pack/multi-pack-index`): a single index
/// over all packfiles, so an object can be found with one lookup instead of one per pack.
/// `write` builds it from every pack; an object stored in several packs is taken from
/// `--preferred-pack` if it is there, else from the newest pack. `verify` checks the file's
/// structure and that every object is where it says. `expire` deletes the packs that no
/// longer contribute any object (everything in them is also in a pack the index prefers)
/// and rewrites the index without them. The file is in git's format (version 1), so git
/// can use it; rgit itself still looks objects up in each pack's own index. `repack`
/// (merging small packs) and bitmaps are not supported. Exit status: 0, 1 when `verify`
/// finds problems, 128 on errors, 129 for a usage error.
pub fn multi_pack_index(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit multi-pack-index write [--preferred-pack=<pack>]\n   or: rgit multi-pack-index verify\n   or: rgit multi-pack-index expire\n";
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        crate::ui::exit(code);
    };
    let mut action: Option<&str> = None;
    let mut preferred: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        match arg {
            "write" | "verify" | "expire" | "repack" if action.is_none() => action = Some(arg),
            "--progress" | "--no-progress" => {}
            "--preferred-pack" => {
                preferred = args.get(i).cloned();
                i += 1;
            }
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            other if other.starts_with("--preferred-pack=") => preferred = Some(other["--preferred-pack=".len()..].to_string()),
            other if other.starts_with("--object-dir") => die("fatal: --object-dir is not supported by rgit multi-pack-index\n", 128),
            other if other.starts_with("--bitmap") || other == "--stdin-packs" || other.starts_with("--refs-snapshot") || other.starts_with("--batch-size") => {
                die(&format!("fatal: {} is not supported by rgit multi-pack-index\n", other), 128)
            }
            other => die(&format!("error: unknown option `{}'\n{}", other.trim_start_matches('-'), USAGE), 129),
        }
    }
    let Some(action) = action else { die(USAGE, 129) };
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git\n", 128);
    }
    let packs = repo_packs();

    match action {
        "write" => {
            if packs.is_empty() {
                eprintln!("error: no pack files to index.");
                return Ok(());
            }
            if let Some(name) = &preferred {
                let stem = name.trim_end_matches(".idx").trim_end_matches(".pack");
                if !packs.iter().any(|p| p.name == stem) {
                    eprintln!("warning: unknown preferred pack: '{}'", name);
                }
            }
            let all: Vec<&RepoPack> = packs.iter().collect();
            store_midx(&write_midx(&all, preferred.as_deref()).0)?;
        }
        "verify" => {
            let Ok(data) = fs::read(midx_path()) else { return Ok(()) };
            let problems = verify_midx(&data, &packs);
            for problem in &problems {
                eprintln!("error: {}", problem);
            }
            if !problems.is_empty() {
                crate::ui::exit(1);
            }
        }
        "expire" => {
            let Ok(data) = fs::read(midx_path()) else { return Ok(()) };
            let (pack_names, objects) = match parse_midx(&data) {
                Ok(parsed) => parsed,
                Err(e) => die(&format!("error: {}\n", e), 128),
            };
            let mut used = vec![0usize; pack_names.len()];
            for (_, pack, _) in &objects {
                used[*pack as usize] += 1;
            }
            let dir = repo::path(".git/objects/pack");
            let mut kept: Vec<&RepoPack> = Vec::new();
            let mut removed = false;
            for (name, count) in pack_names.iter().zip(&used) {
                let stem = name.trim_end_matches(".idx");
                let pack = packs.iter().find(|p| p.name == stem);
                if *count > 0 || dir.join(format!("{}.keep", stem)).exists() {
                    kept.extend(pack);
                    continue;
                }
                for extension in ["pack", "idx", "rev", "bitmap", "mtimes"] {
                    let path = dir.join(format!("{}.{}", stem, extension));
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
                removed = true;
            }
            if removed {
                store_midx(&write_midx(&kept, None).0)?;
                forget_repo_packs();
            }
        }
        _ => die("fatal: multi-pack-index repack is not supported by rgit; use `repack -a -d`\n", 128),
    }
    Ok(())
}
