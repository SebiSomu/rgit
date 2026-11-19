use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use sha1::{Digest, Sha1};
use crate::core::config::get_config_value;
use crate::helpers::format_patch::{split_diff_lines, unified_hunks};
use crate::helpers::merge_tree::merge_file_lines;
use crate::core::index::read_index;

/// git's usage text for `rerere`, printed for `-h` and after usage errors (exit 129).
pub const RERERE_USAGE: &str = "usage: rgit rerere [clear | forget <pathspec>... | diff | status | remaining | gc]\n";

/// Says whether rerere is active, like git's `is_rerere_enabled`: `rerere.enabled` when
/// set, otherwise whether `.git/rr-cache` already exists.
// Used by `rerere`.
pub fn rerere_enabled() -> bool {
    match get_config_value("rerere.enabled").ok().flatten() {
        Some(value) => matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"),
        None => rr_cache_dir().is_dir(),
    }
}

/// `.git/rr-cache`, where recorded conflicts (`preimage`) and their resolutions (`postimage`) live.
// Used by `rerere` (internal to this file's helpers).
fn rr_cache_dir() -> PathBuf {
    crate::core::repo::path(".git/rr-cache")
}

/// The file of one recorded conflict: `.git/rr-cache/<id>/<name>`.
// Used by `rerere` (internal to this file's helpers).
fn rr_file(id: &str, name: &str) -> PathBuf {
    rr_cache_dir().join(id).join(name)
}

/// git's `is_cmarker`: the line starts with seven `marker` characters followed by
/// whitespace (a space is required after `<<<<<<<` and `>>>>>>>`).
// Used by `normalize_conflicts`.
fn is_conflict_marker(line: &[u8], marker: u8) -> bool {
    if line.len() < 8 || line[..7].iter().any(|&b| b != marker) {
        return false;
    }
    if matches!(marker, b'<' | b'>') { line[7] == b' ' } else { matches!(line[7], b' ' | b'\t' | b'\n' | b'\r') }
}

/// Port of git's `handle_path`: rewrites file content with every conflict in canonical
/// form (bare markers, the two sides in sorted order, no diff3 base section) and hashes the
/// sides into the conflict ID that names the conflict in `rr-cache`. Returns the normalized
/// content, the ID, and how many conflicts there were; `None` for a malformed conflict.
// Used by `rerere` (recording, replaying, `forget`, `remaining`).
pub fn normalize_conflicts(content: &[u8]) -> Option<(Vec<u8>, String, usize)> {
    #[derive(PartialEq)]
    enum Side {
        None,
        One,
        Original,
        Two,
    }
    let mut out = Vec::new();
    let mut hasher = Sha1::new();
    let (mut one, mut two): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
    let mut side = Side::None;
    let mut conflicts = 0;
    for line in content.split_inclusive(|&b| b == b'\n') {
        match side {
            Side::None if is_conflict_marker(line, b'<') => side = Side::One,
            Side::None => out.extend_from_slice(line),
            Side::One if is_conflict_marker(line, b'|') => side = Side::Original,
            Side::One | Side::Original if is_conflict_marker(line, b'=') => side = Side::Two,
            Side::One if is_conflict_marker(line, b'<') => return None,
            Side::One => one.extend_from_slice(line),
            Side::Original => {}
            Side::Two if is_conflict_marker(line, b'>') => {
                if one > two {
                    std::mem::swap(&mut one, &mut two);
                }
                conflicts += 1;
                out.extend_from_slice(b"<<<<<<<\n");
                out.extend_from_slice(&one);
                out.extend_from_slice(b"=======\n");
                out.extend_from_slice(&two);
                out.extend_from_slice(b">>>>>>>\n");
                hasher.update(&one);
                hasher.update([0u8]);
                hasher.update(&two);
                hasher.update([0u8]);
                one.clear();
                two.clear();
                side = Side::None;
            }
            Side::Two => two.extend_from_slice(line),
        }
    }
    if side != Side::None {
        return None;
    }
    Some((out, hex::encode(hasher.finalize()), conflicts))
}

/// Reads `.git/MERGE_RR`: the conflicts being tracked as `(conflict id, path)`, each
/// stored as `<id>\t<path>\0`.
// Used by `rerere` (every subcommand).
pub fn read_merge_rr() -> Vec<(String, String)> {
    let Ok(data) = fs::read(crate::core::repo::path(".git/MERGE_RR")) else { return Vec::new() };
    data.split(|&b| b == 0)
        .filter_map(|entry| {
            let text = String::from_utf8_lossy(entry);
            let (id, path) = text.split_once('\t')?;
            Some((id.split('.').next().unwrap_or(id).to_string(), path.to_string()))
        })
        .collect()
}

/// Writes `.git/MERGE_RR` (removing it when nothing is tracked any more).
// Used by `rerere` (recording, `forget`, `clear`).
pub fn write_merge_rr(entries: &[(String, String)]) -> anyhow::Result<()> {
    let path = crate::core::repo::path(".git/MERGE_RR");
    let mut data = Vec::new();
    for (id, file) in entries {
        data.extend_from_slice(format!("{}\t{}", id, file).as_bytes());
        data.push(0);
    }
    fs::write(path, data)?;
    Ok(())
}

/// Lists the tracked files whose working-tree copy currently contains conflict markers,
/// in path order. rgit's index keeps no conflict stages, so the markers are the evidence.
// Used by `rerere` (recording and `remaining`).
pub fn conflicted_paths() -> anyhow::Result<Vec<String>> {
    let tracked: BTreeSet<String> = read_index()?.into_iter().map(|entry| entry.path).collect();
    Ok(tracked.into_iter().filter(|path| fs::read(path).ok().and_then(|content| normalize_conflicts(&content)).is_some_and(|(_, _, conflicts)| conflicts > 0)).collect())
}

/// Port of git's `do_plain_rerere`: records and reuses conflict resolutions. A conflict
/// seen for the first time has its normalized form saved as the preimage; a tracked
/// conflict whose file no longer has markers has that file saved as its resolution; and a
/// conflict that already has a resolution gets it applied (a three-way merge of preimage,
/// current file and postimage), leaving the file resolved in the working tree. Returns
/// the progress messages git prints on standard error.
// Used by `rerere` (no subcommand).
pub fn record_and_replay() -> anyhow::Result<Vec<String>> {
    let mut messages = Vec::new();
    let mut tracked = read_merge_rr();
    for path in conflicted_paths()? {
        if tracked.iter().any(|(_, tracked_path)| *tracked_path == path) {
            continue;
        }
        if let Some((_, id, _)) = fs::read(&path).ok().and_then(|content| normalize_conflicts(&content)) {
            tracked.push((id, path));
        }
    }
    tracked.sort_by(|a, b| a.1.cmp(&b.1));

    let mut remaining = Vec::new();
    for (id, path) in tracked {
        let content = fs::read(&path).unwrap_or_default();
        let normalized = normalize_conflicts(&content);
        let has_conflicts = normalized.as_ref().is_some_and(|(_, _, conflicts)| *conflicts > 0);
        let (preimage, postimage) = (rr_file(&id, "preimage"), rr_file(&id, "postimage"));

        if preimage.exists() && !has_conflicts {
            fs::write(&postimage, &content)?;
            messages.push(format!("Recorded resolution for '{}'.", path));
            continue;
        }
        if preimage.exists() && postimage.exists() {
            let this = normalized.map(|(text, _, _)| text).unwrap_or_default();
            fs::write(rr_file(&id, "thisimage"), &this)?;
            let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
            let merged = merge_file_lines(&text(fs::read(&preimage)?), &text(this), &text(fs::read(&postimage)?), "", "", 7);
            if merged.conflicts.is_empty() {
                fs::write(&path, &merged.content)?;
                messages.push(format!("Resolved '{}' using previous resolution.", path));
                continue;
            }
            remaining.push((id, path));
            continue;
        }
        if let Some((text, _, _)) = normalized.filter(|_| has_conflicts) {
            fs::create_dir_all(rr_cache_dir().join(&id))?;
            fs::write(&preimage, text)?;
            let _ = fs::remove_file(&postimage);
            messages.push(format!("Recorded preimage for '{}'", path));
        }
        remaining.push((id, path));
    }
    write_merge_rr(&remaining)?;
    Ok(messages)
}

/// `rerere diff`: for each tracked conflict, a unified diff from its recorded preimage to
/// the file as it is now, showing what the resolution in progress has changed.
// Used by `rerere diff`.
pub fn rerere_diff() -> String {
    let mut out = String::new();
    for (id, path) in read_merge_rr() {
        let (Ok(before), Ok(now)) = (fs::read(rr_file(&id, "preimage")), fs::read(&path)) else { continue };
        out.push_str(&format!("--- a/{}\n+++ b/{}\n", path, path));
        for line in unified_hunks(&split_diff_lines(&before), &split_diff_lines(&now)).0.lines() {
            // git prints these hunk headers without the function context.
            match line.strip_prefix("@@ ").and_then(|rest| rest.find(" @@").map(|end| &line[..end + 6])) {
                Some(header) => out.push_str(header),
                None => out.push_str(line),
            }
            out.push('\n');
        }
    }
    out
}

/// `rerere forget`: drops the recorded resolution of the conflict in each path (found
/// through `MERGE_RR`, or from the conflict markers currently in the file) so the next
/// resolution is recorded afresh. Returns git's messages; errors name paths with nothing
/// recorded.
// Used by `rerere forget`.
pub fn rerere_forget(paths: &[String]) -> anyhow::Result<Vec<String>> {
    let mut tracked = read_merge_rr();
    let mut messages = Vec::new();
    for path in paths {
        let path = path.trim_start_matches("./").to_string();
        let current = fs::read(&path).ok().and_then(|content| normalize_conflicts(&content)).filter(|(_, _, conflicts)| *conflicts > 0);
        // A file that rerere already resolved has no markers left: find its conflict by the recorded resolution.
        let by_resolution = || {
            let content = fs::read(&path).ok()?;
            fs::read_dir(rr_cache_dir()).ok()?.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).find(|id| fs::read(rr_file(id, "postimage")).is_ok_and(|post| post == content))
        };
        let id = tracked.iter().find(|(_, tracked_path)| *tracked_path == path).map(|(id, _)| id.clone()).or_else(|| current.as_ref().map(|(_, id, _)| id.clone())).or_else(by_resolution);
        let Some(id) = id.filter(|id| rr_file(id, "preimage").exists() || rr_file(id, "postimage").exists()) else {
            anyhow::bail!("error: no remembered resolution for '{}'", path);
        };
        let _ = fs::remove_file(rr_file(&id, "postimage"));
        if let Some((text, _, _)) = current {
            fs::write(rr_file(&id, "preimage"), text)?;
            messages.push(format!("Updated preimage for '{}'", path));
        }
        messages.push(format!("Forgot resolution for '{}'", path));
        if !tracked.iter().any(|(_, tracked_path)| *tracked_path == path) {
            tracked.push((id, path));
        }
    }
    tracked.sort_by(|a, b| a.1.cmp(&b.1));
    write_merge_rr(&tracked)?;
    Ok(messages)
}

/// `rerere clear`: forgets the conflicts being tracked, deleting the recordings that have
/// no resolution yet, and removes `MERGE_RR`.
// Used by `rerere clear`.
pub fn rerere_clear() {
    for (id, _) in read_merge_rr() {
        if !rr_file(&id, "postimage").exists() {
            let _ = fs::remove_dir_all(rr_cache_dir().join(&id));
        }
    }
    let _ = fs::remove_file(crate::core::repo::path(".git/MERGE_RR"));
}

/// `rerere gc`: prunes old recordings: unresolved ones after `gc.rerereUnresolved` days
/// (15) and resolved ones after `gc.rerereResolved` days (60), judged by file time.
// Used by `rerere gc`.
pub fn rerere_gc() {
    let days = |key: &str, default: u64| get_config_value(key).ok().flatten().and_then(|v| v.parse::<u64>().ok()).unwrap_or(default);
    let (unresolved, resolved) = (days("gc.rerereunresolved", 15), days("gc.rerereresolved", 60));
    let older_than = |path: PathBuf, days: u64| {
        fs::metadata(path).and_then(|meta| meta.modified()).ok().and_then(|time| SystemTime::now().duration_since(time).ok()).is_some_and(|age| age > Duration::from_secs(days * 86400))
    };
    let Ok(entries) = fs::read_dir(rr_cache_dir()) else { return };
    for entry in entries.flatten() {
        let id = entry.file_name().to_string_lossy().into_owned();
        let expired = if rr_file(&id, "postimage").exists() { older_than(rr_file(&id, "postimage"), resolved) } else { older_than(rr_file(&id, "preimage"), unresolved) };
        if expired {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
