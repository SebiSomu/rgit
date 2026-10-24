use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use crate::helpers::rev_list::parse_approx_date;
use crate::objects::ObjectDirCruft;

/// Turns `prune --expire=<time>` into the cutoff `plan_gc` uses (objects modified at or after
/// it are kept): `None` for `never`; otherwise git's approximate dates (`now`, `all`,
/// `2.weeks.ago`, `2024-01-01`, a Unix timestamp), where an object exactly as old as the
/// date is pruned. Without `--expire` every unreachable object goes.
// Used by the `prune` command.
pub fn prune_expiry_cutoff(spec: Option<&str>, now: u64) -> Result<Option<u64>> {
    let Some(spec) = spec else {
        return Ok(Some(u64::MAX));
    };
    match spec.trim().to_ascii_lowercase().as_str() {
        "never" => Ok(None),
        "all" | "now" => Ok(Some(now + 1)),
        _ => {
            let ts = parse_approx_date(spec).ok_or_else(|| anyhow::anyhow!("fatal: malformed expiration date '{}'", spec))?;
            Ok(Some((ts.max(0) as u64).saturating_add(1)))
        }
    }
}

fn modified_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn sorted_entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut entries: Vec<(String, PathBuf)> = fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect())
        .unwrap_or_default();
    entries.sort();
    entries
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Finds the files in `.git/objects` that aren't loose objects, in the order git's `prune`
/// visits them: each fan-out directory (`00`..`ff`) in name order, where `tmp_obj_*` files
/// are stale temporaries and anything else is a stray "bad sha1 file", then `tmp_*` files in
/// `objects/` and in `objects/pack/`. Each entry comes with the key it sorts under among
/// the loose objects (`<xx><rest of name>` inside a fan-out directory).
// Used by the `prune` command.
pub fn list_object_dir_cruft() -> Result<Vec<(String, ObjectDirCruft)>> {
    let root = crate::repo::path(".git/objects");
    let mut cruft = Vec::new();
    for (name, dir) in sorted_entries(&root) {
        if !is_hex(&name, 2) || !dir.is_dir() {
            continue;
        }
        for (file, path) in sorted_entries(&dir) {
            if is_hex(&file, 38) {
                continue;
            }
            let key = format!("{}{}", name, file);
            if file.starts_with("tmp_obj_") {
                cruft.push((key, ObjectDirCruft::StaleTemp { modified: modified_secs(&path), path }));
            } else {
                cruft.push((key, ObjectDirCruft::Bad { path }));
            }
        }
    }
    for dir in [root.clone(), root.join("pack")] {
        for (file, path) in sorted_entries(&dir) {
            if file.starts_with("tmp_") && path.is_file() {
                cruft.push((format!("~{:08}", cruft.len()), ObjectDirCruft::StaleTemp { modified: modified_secs(&path), path }));
            }
        }
    }
    Ok(cruft)
}

/// Removes every empty fan-out directory (`.git/objects/xx/`).
// Used by the `prune` command.
pub fn remove_empty_fanout_dirs() {
    for (name, dir) in sorted_entries(&crate::repo::path(".git/objects")) {
        if is_hex(&name, 2) {
            let _ = fs::remove_dir(dir);
        }
    }
}
