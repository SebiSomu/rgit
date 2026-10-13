use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use crate::helpers::fetch::local_has_object;
use crate::helpers::read_object;
use crate::helpers::reflog::{expire_reflog, list_reflog_refs, read_reflog, NULL_HASH};
use crate::index::read_index;
use crate::objects::{GcReport, LooseObject};

/// State files under `.git/` whose contents name commits that must stay alive: refs HEAD
/// points at, stashes, and the bookkeeping of merges, cherry-picks, reverts, rebases and
/// bisects in progress. A missing object named here means the repository is damaged, so the
/// run is aborted rather than risk pruning around the hole.
///
/// ANY NEW FEATURE THAT SAVES A COMMIT HASH IN A `.git/` FILE MUST BE ADDED HERE, or `gc` will
/// treat what it points at as garbage.
const GC_STRICT_STATE_FILES: &[&str] = &[
    "HEAD",
    "packed-refs",
    "STASH_LIST",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "REBASE_ORIG_HEAD",
    "REBASE_ONTO",
    "REBASE_TODO",
    "BISECT_START",
    "BISECT_BAD",
    "BISECT_GOOD",
    "BISECT_SKIP",
];

/// State files that only record history ("what was fetched last"). Their objects are kept if
/// present, but a missing one is not an error.
const GC_LENIENT_STATE_FILES: &[&str] = &["FETCH_HEAD", "ORIG_HEAD"];

/// Turns an expiry setting (`<days>`, `now` or `never`) into the Unix timestamp before which
/// things count as expired, or `None` if nothing ever expires. `now` expires everything that
/// already exists.
// Used by the `gc` command.
pub fn expiry_cutoff(spec: &str, now: u64) -> Result<Option<u64>> {
    match spec.trim() {
        "never" => Ok(None),
        "now" => Ok(Some(now + 1)),
        days => {
            let days: u64 = days.parse().map_err(|_| {
                anyhow::anyhow!("fatal: invalid expiry '{}' (expected a number of days, 'now' or 'never')", spec)
            })?;
            Ok(Some(now.saturating_sub(days.saturating_mul(86_400))))
        }
    }
}

/// Refuses to run in a repository that has packfiles. `gc` only understands loose objects, and
/// objects inside a pack would look unreachable and missing, so nothing is safe to prune.
// Used by `plan_gc`.
fn check_no_pack_files() -> Result<()> {
    let pack_dir = Path::new(".git/objects/pack");
    if !pack_dir.is_dir() {
        return Ok(());
    }
    let has_pack = fs::read_dir(pack_dir)
        .with_context(|| format!("Failed to read {}", pack_dir.display()))?
        .flatten()
        .any(|e| e.path().extension().is_some_and(|ext| ext == "pack"));
    if has_pack {
        anyhow::bail!(
            "fatal: this repository has packfiles (.git/objects/pack/*.pack); rgit gc only handles loose objects and will not touch it"
        );
    }
    Ok(())
}

/// Extracts every 40-hex-digit token from a piece of text (a ref file, a todo list, a
/// `FETCH_HEAD`), whatever the surrounding format is.
// Used by `hashes_in_file`.
fn hashes_in_text(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() == 40 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Returns the hashes named in a file, or nothing if the file doesn't exist.
// Used by `collect_strict_gc_roots` and `collect_lenient_gc_roots`.
fn hashes_in_file(path: &Path) -> Result<Vec<String>> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(hashes_in_text(&text))
}

/// Collects every ref file under a directory (`.git/refs/heads`, `.git/refs/remotes`,
/// `.git/refs/tags`, ...) and returns the hashes they hold.
// Used by `collect_strict_gc_roots`.
fn hashes_in_refs_dir(dir: &Path, out: &mut Vec<String>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            hashes_in_refs_dir(&path, out)?;
        } else {
            out.extend(hashes_in_file(&path)?);
        }
    }
    Ok(())
}

/// Gathers the commits and blobs that must exist: every ref (branches, remote-tracking
/// branches, tags), HEAD, the stash list, the in-progress-operation state files, and every
/// blob staged in the index (staged content that was never committed lives only there).
// Used by `plan_gc`.
pub fn collect_strict_gc_roots() -> Result<Vec<String>> {
    let mut roots = Vec::new();

    for name in GC_STRICT_STATE_FILES {
        roots.extend(hashes_in_file(&Path::new(".git").join(name))?);
    }
    hashes_in_refs_dir(Path::new(".git/refs"), &mut roots)?;

    for entry in read_index().unwrap_or_default() {
        roots.push(hex::encode(entry.hash));
    }

    Ok(roots)
}

/// Gathers the best-effort roots: the history-only state files and every hash in a reflog entry
/// that hasn't expired (`reflog_cutoff`), which keeps commits that were reset, amended or
/// rebased away recoverable until their entries age out.
// Used by `plan_gc`.
pub fn collect_lenient_gc_roots(reflog_cutoff: Option<u64>) -> Result<Vec<String>> {
    let mut roots = Vec::new();

    for name in GC_LENIENT_STATE_FILES {
        roots.extend(hashes_in_file(&Path::new(".git").join(name))?);
    }

    for ref_name in list_reflog_refs()? {
        for entry in read_reflog(&ref_name)? {
            if reflog_cutoff.is_some_and(|cutoff| entry.timestamp < cutoff) {
                continue;
            }
            for hash in [&entry.old_hash, &entry.new_hash] {
                if hash != NULL_HASH {
                    roots.push(hash.clone());
                }
            }
        }
    }

    Ok(roots)
}

/// Marks everything reachable from `roots` (commits -> parents and trees, trees -> subtrees
/// and blobs, tags -> their target) by adding it to `visited`. With `strict`, an object that
/// can't be read is an error; without it, such objects are skipped. Submodule entries
/// (mode 160000) point into other repositories and are ignored.
// Used by `plan_gc`.
pub fn mark_reachable(roots: &[String], visited: &mut HashSet<String>, strict: bool) -> Result<()> {
    let mut stack: Vec<(String, Option<&'static str>)> = roots.iter().map(|h| (h.clone(), None)).collect();

    while let Some((hash, kind_hint)) = stack.pop() {
        if !visited.insert(hash.clone()) {
            continue;
        }

        // A blob's contents never matter, so only check that it exists.
        if kind_hint == Some("blob") {
            if strict && !local_has_object(&hash) {
                anyhow::bail!("fatal: missing blob object {}; refusing to prune a damaged repository", hash);
            }
            continue;
        }

        let (kind, content) = match read_object(&hash) {
            Ok(found) => found,
            Err(err) if strict => {
                return Err(err.context(format!(
                    "fatal: cannot read reachable object {}; refusing to prune a damaged repository",
                    hash
                )));
            }
            Err(_) => continue,
        };

        match kind.as_str() {
            "commit" => {
                for line in String::from_utf8_lossy(&content).lines() {
                    if line.is_empty() {
                        break;
                    }
                    if let Some(tree) = line.strip_prefix("tree ") {
                        stack.push((tree.to_string(), Some("tree")));
                    } else if let Some(parent) = line.strip_prefix("parent ") {
                        stack.push((parent.to_string(), Some("commit")));
                    }
                }
            }
            "tree" => {
                let mut pos = 0;
                while pos < content.len() {
                    let space = content[pos..].iter().position(|&b| b == b' ').context("Missing mode separator in tree object")? + pos;
                    let nul = content[space..].iter().position(|&b| b == 0).context("Missing name terminator in tree object")? + space;
                    let hash_end = nul + 21;
                    if hash_end > content.len() {
                        anyhow::bail!("Corrupted tree object {}: incomplete hash", hash);
                    }
                    let mode = String::from_utf8_lossy(&content[pos..space]).to_string();
                    let child = hex::encode(&content[nul + 1..hash_end]);
                    match mode.as_str() {
                        "160000" => {}
                        "40000" | "040000" => stack.push((child, Some("tree"))),
                        _ => stack.push((child, Some("blob"))),
                    }
                    pos = hash_end;
                }
            }
            "tag" => {
                if let Some(target) = String::from_utf8_lossy(&content).lines().find_map(|l| l.strip_prefix("object ")) {
                    stack.push((target.to_string(), None));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

/// Lists every loose object in `.git/objects/xx/`, with its size and last-modified time. Only
/// two-hex-digit directories and 38-hex-digit file names count; `pack/`, `info/` and stray
/// files are ignored.
// Used by `plan_gc`.
pub fn list_loose_objects() -> Result<Vec<LooseObject>> {
    let root = Path::new(".git/objects");
    if !root.is_dir() {
        anyhow::bail!("fatal: not an rgit repository (no .git/objects)");
    }

    let is_hex = |s: &str, len: usize| s.len() == len && s.chars().all(|c| c.is_ascii_hexdigit());
    let mut objects = Vec::new();

    for dir in fs::read_dir(root).context("Failed to read .git/objects")? {
        let dir = dir?;
        let prefix = dir.file_name().to_string_lossy().to_string();
        if !is_hex(&prefix, 2) || !dir.path().is_dir() {
            continue;
        }
        for file in fs::read_dir(dir.path()).with_context(|| format!("Failed to read {}", dir.path().display()))? {
            let file = file?;
            let rest = file.file_name().to_string_lossy().to_string();
            if !is_hex(&rest, 38) {
                continue;
            }
            let metadata = file.metadata()?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            objects.push(LooseObject {
                hash: format!("{}{}", prefix, rest).to_ascii_lowercase(),
                path: file.path(),
                size: metadata.len(),
                modified,
            });
        }
    }

    objects.sort_by(|a, b| a.hash.cmp(&b.hash));
    Ok(objects)
}

/// Works out what a `gc` would prune, without changing anything. Everything reachable from the
/// roots is kept (the strict roots must be intact, the reflog roots needn't be); of the rest,
/// anything modified at or after `prune_cutoff` is kept as too new to judge, and so is
/// everything such a recent object refers to, so a kept commit never loses its tree. A `None`
/// cutoff (`--prune=never`) prunes nothing.
// Used by the `gc` command.
pub fn plan_gc(prune_cutoff: Option<u64>, reflog_cutoff: Option<u64>) -> Result<GcReport> {
    check_no_pack_files()?;

    let mut keep: HashSet<String> = HashSet::new();
    mark_reachable(&collect_strict_gc_roots()?, &mut keep, true)?;
    mark_reachable(&collect_lenient_gc_roots(reflog_cutoff)?, &mut keep, false)?;

    let loose = list_loose_objects()?;
    let total_objects = loose.len();
    let reachable_objects = loose.iter().filter(|o| keep.contains(&o.hash)).count();
    let unreachable: Vec<LooseObject> = loose.into_iter().filter(|o| !keep.contains(&o.hash)).collect();
    let unreachable_objects = unreachable.len();

    let Some(cutoff) = prune_cutoff else {
        return Ok(GcReport {
            total_objects,
            reachable_objects,
            unreachable_objects,
            kept_unreachable: unreachable_objects,
            pruned: Vec::new(),
            reflog_entries_expired: 0,
        });
    };

    let recent: Vec<String> = unreachable.iter().filter(|o| o.modified >= cutoff).map(|o| o.hash.clone()).collect();
    mark_reachable(&recent, &mut keep, false)?;

    let pruned: Vec<LooseObject> = unreachable.into_iter().filter(|o| !keep.contains(&o.hash)).collect();

    Ok(GcReport {
        total_objects,
        reachable_objects,
        unreachable_objects,
        kept_unreachable: unreachable_objects - pruned.len(),
        pruned,
        reflog_entries_expired: 0,
    })
}

/// Expires old entries from every reflog and returns how many went (or, with `dry_run`, would
/// go). A `None` cutoff expires nothing.
// Used by the `gc` command.
pub fn expire_all_reflogs(cutoff: Option<u64>, dry_run: bool) -> Result<usize> {
    let Some(cutoff) = cutoff else {
        return Ok(0);
    };

    let mut total = 0;
    for ref_name in list_reflog_refs()? {
        total += if dry_run {
            read_reflog(&ref_name)?.iter().filter(|e| e.timestamp < cutoff).count()
        } else {
            expire_reflog(&ref_name, Some(cutoff))?
        };
    }
    Ok(total)
}

/// Deletes one loose object file (clearing a read-only flag first if needed, as objects copied
/// from a real git repository are read-only, which blocks deletion on Windows) and removes its
/// `xx/` directory if that leaves it empty.
// Used by the `gc` command.
pub fn remove_loose_object(object: &LooseObject) -> Result<()> {
    if fs::remove_file(&object.path).is_err() {
        let mut permissions = fs::metadata(&object.path)
            .with_context(|| format!("Failed to stat {}", object.path.display()))?
            .permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(&object.path, permissions)
            .with_context(|| format!("Failed to make {} writable", object.path.display()))?;
        fs::remove_file(&object.path)
            .with_context(|| format!("Failed to delete {}", object.path.display()))?;
    }

    if let Some(dir) = object.path.parent() {
        let is_empty = fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(false);
        if is_empty {
            let _ = fs::remove_dir(dir);
        }
    }
    Ok(())
}

/// Formats a byte count for humans (`512 B`, `3.4 KiB`, `1.2 MiB`).
// Used by the `gc` command.
pub fn format_byte_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// The path of a loose object for a hash, as `list_loose_objects` would report it.
// Used by the `gc` command's tests and diagnostics.
#[allow(dead_code)]
pub fn loose_object_path(hash: &str) -> PathBuf {
    Path::new(".git/objects").join(&hash[..2]).join(&hash[2..])
}