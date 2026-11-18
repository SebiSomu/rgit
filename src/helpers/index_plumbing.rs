use std::fs;

use crate::helpers::objects::{hash_content, write_object};
use crate::index::{build_entry, IndexEntry};
use crate::objects::{UpdateIndexFlags, IndexFileState};

/// The index mode a file on disk would get: `100755` when executable (Unix only), else
/// `100644`; on systems without an executable bit the entry's current mode is kept.
// Used by `worktree_state` and `update_index_path`.
pub fn worktree_file_mode(metadata: &fs::Metadata, current: Option<u32>) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = current;
        if metadata.permissions().mode() & 0o111 != 0 { 0o100755 } else { 0o100644 }
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        current.filter(|mode| mode & 0o170000 == 0o100000).unwrap_or(0o100644)
    }
}

/// Compares an index entry with its file in the working tree. The cached size and
/// modification time are trusted when they match; otherwise the file is hashed, so an entry
/// whose stat data is merely stale (or was never recorded) still counts as clean.
// Used by `update-index --refresh`, `checkout-index`, `read-tree -u`, `diff-files` and `diff-index`.
pub fn worktree_state(entry: &IndexEntry) -> IndexFileState {
    let metadata = match fs::metadata(&entry.path) {
        Ok(metadata) if metadata.is_file() => metadata,
        // A skip-worktree entry is meant to be absent (sparse checkout): that is not a change.
        _ if entry.skip_worktree => return IndexFileState::Clean,
        _ => return IndexFileState::Missing,
    };
    let mode = if entry.mode & 0o170000 == 0o100000 { worktree_file_mode(&metadata, Some(entry.mode)) } else { entry.mode };
    if mode != entry.mode {
        return IndexFileState::Modified(mode);
    }
    let fresh = build_entry(&entry.path, entry.hash, &metadata);
    let stat_matches = entry.size == fresh.size && entry.mtime_secs == fresh.mtime_secs && entry.mtime_nsecs == fresh.mtime_nsecs && (entry.mtime_secs != 0 || entry.mtime_nsecs != 0);
    if stat_matches {
        return IndexFileState::Clean;
    }
    match fs::read(&entry.path) {
        Ok(content) if hash_content("blob", &content) == entry.hash => IndexFileState::Clean,
        Ok(_) => IndexFileState::Modified(mode),
        Err(_) => IndexFileState::Missing,
    }
}

/// Turns a path argument into the form index paths have: forward slashes, no `./`, no empty
/// or `.` components, `..` resolved. `Err` holds git's message when the path leaves the
/// repository; `Ok(None)` means git would say `Ignoring path` (inside `.git`, or empty).
// Used by `update-index`, `checkout-index` and the `diff-*` commands.
pub fn normalize_index_path(arg: &str) -> Result<Option<String>, String> {
    let unified = arg.replace('\\', "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    let root = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
                    return Err(format!("fatal: '{}' is outside repository at '{}'", arg, root));
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() || parts.iter().any(|p| p.eq_ignore_ascii_case(".git")) {
        return Ok(None);
    }
    Ok(Some(parts.join("/")))
}

/// Whether `path` clashes with an existing entry as file versus directory: an entry lives
/// below `path`, or `path` lies below an existing entry.
// Used by `add_cache_entry`.
fn df_conflict(entries: &[IndexEntry], path: &str) -> bool {
    let below = format!("{}/", path);
    entries.iter().any(|e| e.path.starts_with(&below) || path.starts_with(&format!("{}/", e.path)))
}

/// Puts an entry into the index like git's `add_index_entry`: an entry with the same path is
/// replaced; a new path needs `--add`; a file/directory clash is an error unless `--replace`,
/// which removes the entries in the way. Errors are git's messages (without the final
/// `fatal:` line the caller adds).
// Used by `update-index` (paths, `--cacheinfo`, `--index-info`).
pub fn add_cache_entry(entries: &mut Vec<IndexEntry>, entry: IndexEntry, flags: &UpdateIndexFlags) -> Result<(), String> {
    let cannot_add = format!("error: {}: cannot add to the index - missing --add option?", entry.path);
    if let Some(existing) = entries.iter_mut().find(|e| e.path == entry.path) {
        *existing = entry;
        return Ok(());
    }
    if !flags.add {
        return Err(cannot_add);
    }
    if df_conflict(entries, &entry.path) {
        if !flags.replace {
            return Err(format!("error: '{}' appears as both a file and as a directory\n{}", entry.path, cannot_add));
        }
        let below = format!("{}/", entry.path);
        let path = entry.path.clone();
        entries.retain(|e| !e.path.starts_with(&below) && !path.starts_with(&format!("{}/", e.path)));
    }
    entries.push(entry);
    Ok(())
}

/// An index entry without stat data, as `--cacheinfo` and tree-reading commands create them.
// Used by `update-index --cacheinfo` / `--index-info` and `read-tree`.
pub fn bare_index_entry(path: &str, hash: [u8; 20], mode: u32) -> IndexEntry {
    IndexEntry { ctime_secs: 0, ctime_nsecs: 0, mtime_secs: 0, mtime_nsecs: 0, dev: 0, ino: 0, mode, uid: 0, gid: 0, size: 0, hash, path: path.to_string(), skip_worktree: false, stage: 0 }
}

/// git's `create_ce_mode`: any mode becomes one an index entry can have — `120000` for
/// symlinks, `160000` for directories and submodule links, else `100755` or `100644`.
// Used by `update-index --cacheinfo` / `--index-info`.
pub fn index_mode_from(mode: u32) -> u32 {
    match mode & 0o170000 {
        0o120000 => 0o120000,
        0o040000 | 0o160000 => 0o160000,
        _ if mode & 0o100 != 0 => 0o100755,
        _ => 0o100644,
    }
}

/// Updates one path from the working tree like `update-index <file>`: an unchanged file is
/// left alone, a changed or new one is hashed (and written to the object store unless
/// `--info-only`) and staged, a missing one is removed with `--remove`. Errors are git's
/// `error:` lines; the caller adds `fatal: Unable to process path <path>`.
// Used by `update-index` (path arguments, `--stdin`, `--again`).
pub fn update_index_path(entries: &mut Vec<IndexEntry>, path: &str, flags: &UpdateIndexFlags) -> Result<(), String> {
    let metadata = match fs::symlink_metadata(path).and_then(|m| if m.file_type().is_symlink() { fs::metadata(path) } else { Ok(m) }) {
        Ok(metadata) => metadata,
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory) => {
            if !flags.remove {
                return Err(format!("error: {}: does not exist and --remove not passed", path));
            }
            entries.retain(|e| e.path != path);
            return Ok(());
        }
        Err(e) => return Err(format!("error: lstat(\"{}\"): {}", path, e)),
    };
    if metadata.is_dir() {
        return Err(format!("error: {}: is a directory - add individual files instead", path));
    }
    let existing = entries.iter().find(|e| e.path == path);
    if existing.is_some_and(|e| worktree_state(e) == IndexFileState::Clean) {
        return Ok(());
    }
    let content = fs::read(path).map_err(|e| format!("error: open(\"{}\"): {}", path, e))?;
    let hash = hash_content("blob", &content);
    let mut entry = build_entry(path, hash, &metadata);
    entry.mode = worktree_file_mode(&metadata, existing.map(|e| e.mode));
    // Check that the entry can go in before writing the blob.
    let mut probe = entries.clone();
    add_cache_entry(&mut probe, entry.clone(), flags)?;
    if !flags.info_only {
        write_object("blob", &content).map_err(|e| format!("error: {}: {}", path, e))?;
    }
    *entries = probe;
    Ok(())
}

/// Refreshes the cached stat data of every entry whose file still has the staged content, and
/// returns the paths that need an update (changed, or missing unless `ignore_missing`).
// Used by `update-index --refresh` / `--really-refresh`.
pub fn refresh_index(entries: &mut [IndexEntry], ignore_missing: bool) -> Vec<String> {
    let mut stale = Vec::new();
    for entry in entries.iter_mut() {
        match worktree_state(entry) {
            IndexFileState::Clean => {
                if let Ok(metadata) = fs::metadata(&entry.path) {
                    let mode = entry.mode;
                    *entry = build_entry(&entry.path, entry.hash, &metadata);
                    entry.mode = mode;
                }
            }
            IndexFileState::Missing if ignore_missing => {}
            _ => stale.push(entry.path.clone()),
        }
    }
    stale
}

/// Writes `content` to a new file named `.merge_file_XXXXXX` (six random characters) in the
/// current directory, like git's temporary checkout files, and returns the name.
// Used by `unpack-file` and `checkout-index --temp`.
pub fn write_merge_temp_file(content: &[u8]) -> std::io::Result<String> {
    use std::io::Write;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let clock = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let mut seed = clock ^ ((std::process::id() as u64) << 32) ^ COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed).wrapping_mul(0x9E3779B97F4A7C15);
    let mut last_error = None;
    for _ in 0..100 {
        let mut suffix = String::new();
        for _ in 0..6 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            suffix.push(ALPHABET[((seed >> 33) % ALPHABET.len() as u64) as usize] as char);
        }
        let name = format!(".merge_file_{}", suffix);
        match fs::OpenOptions::new().write(true).create_new(true).open(&name) {
            Ok(mut file) => {
                file.write_all(content)?;
                return Ok(name);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last_error = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last_error.unwrap_or_else(|| std::io::Error::other("no free temporary file name")))
}

/// Writes an index entry's blob to `target` (creating parent directories) and sets the
/// executable bit from the entry's mode. With `force`, a file standing where a directory is
/// needed, or a directory standing where the file goes, is removed first; without it those
/// are errors, worded like git's.
// Used by `checkout-index` and `read-tree -u`.
pub fn write_entry_to_worktree(entry: &IndexEntry, target: &std::path::Path, force: bool) -> Result<(), String> {
    let (_, content) = crate::helpers::objects::read_object(&hex::encode(entry.hash)).map_err(|_| format!("error: unable to read sha1 file of {} ({})", entry.path, hex::encode(entry.hash)))?;
    // Every leading directory must be a directory.
    let mut ancestors: Vec<&std::path::Path> = target.ancestors().skip(1).filter(|p| !p.as_os_str().is_empty()).collect();
    ancestors.reverse();
    for dir in ancestors {
        match fs::symlink_metadata(dir) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) if force => {
                fs::remove_file(dir).map_err(|e| format!("fatal: cannot create directory at '{}': {}", dir.display(), e))?;
                fs::create_dir(dir).map_err(|e| format!("fatal: cannot create directory at '{}': {}", dir.display(), e))?;
            }
            Ok(_) => return Err(format!("fatal: cannot create directory at '{}': File exists", dir.display().to_string().replace('\\', "/"))),
            Err(_) => fs::create_dir(dir).map_err(|e| format!("fatal: cannot create directory at '{}': {}", dir.display(), e))?,
        }
    }
    if target.is_dir() {
        if !force {
            return Err(format!("error: {} is a directory", target.display().to_string().replace('\\', "/")));
        }
        fs::remove_dir_all(target).map_err(|e| format!("error: unable to remove directory {}: {}", target.display(), e))?;
    }
    // Replace rather than write through, so a hard link or read-only file does not get in the way.
    let _ = fs::remove_file(target);
    fs::write(target, &content).map_err(|e| format!("error: unable to create file {}: {}", target.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if entry.mode & 0o111 != 0 { 0o755 } else { 0o644 };
        fs::set_permissions(target, fs::Permissions::from_mode(mode)).map_err(|e| format!("error: unable to set permissions of {}: {}", target.display(), e))?;
    }
    Ok(())
}
