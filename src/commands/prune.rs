use anyhow::Result;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::helpers::gc::{plan_gc, remove_loose_object};
use crate::helpers::prune::{list_object_dir_cruft, prune_expiry_cutoff, remove_empty_fanout_dirs};
use crate::helpers::read_object;
use crate::helpers::rev_parse::resolve_rev;
use crate::objects::{LooseObject, ObjectDirCruft};
use crate::repo;

/// One thing `prune` visits: an unreachable loose object or a non-object file.
enum PruneItem<'a> {
    Object(&'a LooseObject),
    Cruft(ObjectDirCruft),
}

/// `rgit prune [-n] [-v] [--progress] [--expire <time>] [--] [<head>...]`
///
/// Deletes loose objects that nothing reaches: no branch, tag, remote-tracking branch, HEAD,
/// index, stash, operation in progress, reflog entry (reflogs are not expired here), or
/// `<head>` given on the command line. Unlike `gc`, every unreachable object goes by default;
/// with `--expire`, objects newer than the date (and whatever they refer to) are kept. Stale
/// temporary files from interrupted writes are removed too, and other stray files in the
/// object directories are reported. Silent unless `-n` (show what would be removed and change
/// nothing) or `-v` is given, which print `<hash> <type>` per object, like git. If a
/// reachable object is missing, nothing is deleted.
pub fn prune(dry_run: bool, verbose: bool, expire: Option<String>, heads: Vec<String>) -> Result<()> {
    if !repo::exists() {
        anyhow::bail!("fatal: not a git repository (or any of the parent directories): .git");
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let cutoff = prune_expiry_cutoff(expire.as_deref(), now).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(128);
    });

    let mut extra_roots = Vec::new();
    for head in &heads {
        match resolve_rev(head) {
            Ok(hash) => extra_roots.push(hash),
            Err(_) => {
                eprintln!("fatal: unrecognized argument: {}", head);
                std::process::exit(128);
            }
        }
    }

    let report = plan_gc(cutoff, None, &extra_roots)?;
    let show = dry_run || verbose;

    // Objects and other files are handled together, fan-out directory by directory, as git does.
    let mut items: Vec<(String, PruneItem)> = report.pruned.iter().map(|o| (o.hash.clone(), PruneItem::Object(o))).collect();
    items.extend(list_object_dir_cruft()?.into_iter().map(|(key, c)| (key, PruneItem::Cruft(c))));
    items.sort_by(|a, b| a.0.cmp(&b.0));

    for (_, item) in items {
        match item {
            PruneItem::Object(object) => {
                if show {
                    let kind = read_object(&object.hash).map(|(k, _)| k).unwrap_or_else(|_| "unknown".to_string());
                    println!("{} {}", object.hash, kind);
                }
                if !dry_run {
                    remove_loose_object(object)?;
                }
            }
            PruneItem::Cruft(ObjectDirCruft::StaleTemp { path, modified }) => {
                if cutoff.is_none_or(|c| modified >= c) {
                    continue;
                }
                if show {
                    println!("Removing stale temporary file {}", path.display().to_string().replace('\\', "/"));
                }
                if !dry_run {
                    fs::remove_file(&path)?;
                }
            }
            PruneItem::Cruft(ObjectDirCruft::Bad { path }) => {
                eprintln!("bad sha1 file: {}", path.display().to_string().replace('\\', "/"));
            }
        }
    }
    if !dry_run {
        remove_empty_fanout_dirs();
    }
    Ok(())
}
