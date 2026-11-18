use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use anyhow::Context;

use crate::helpers::checkout::{build_index_entries_for_tree, ensure_working_tree_clean};
use crate::helpers::commit::commit_parents;
use crate::helpers::objects::{flatten_tree, read_object, tree_hash_of_commit};
use crate::index::{write_index, IndexEntry};
use crate::objects::StashEntry;
use crate::refs;

pub const STASH_LIST_PATH: &str = ".git/STASH_LIST";

pub fn read_lines_set(path: impl AsRef<Path>) -> anyhow::Result<Vec<String>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

pub fn append_line(path: impl AsRef<Path>, line: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    let path = path.as_ref();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path.display()))?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Reads all stash entries, most-recent-first (`stash@{0}` == `list[0]`).
pub fn read_stash_list() -> anyhow::Result<Vec<StashEntry>> {
    if !crate::repo::path(STASH_LIST_PATH).exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(crate::repo::path(STASH_LIST_PATH)).context("Failed to read .git/STASH_LIST")?;
    let mut entries: Vec<StashEntry> = content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let hash = parts.next()?.to_string();
            let message = parts.next().unwrap_or("").to_string();
            Some(StashEntry { hash, message })
        })
        .collect();

    entries.reverse();
    Ok(entries)
}

/// Overwrites the stash list. `entries` must be newest-first (index 0 ==
/// `stash@{0}`), matching what `read_stash_list` returns.
pub fn write_stash_list(entries: &[StashEntry]) -> anyhow::Result<()> {
    if entries.is_empty() {
        if crate::repo::path(STASH_LIST_PATH).exists() {
            fs::remove_file(crate::repo::path(STASH_LIST_PATH)).context("Failed to remove .git/STASH_LIST")?;
        }
        return Ok(());
    }

    let mut lines: Vec<String> = entries.iter().map(|e| format!("{} {}", e.hash, e.message)).collect();
    lines.reverse();
    let mut content = lines.join("\n");
    content.push('\n');
    fs::write(crate::repo::path(STASH_LIST_PATH), content).context("Failed to write .git/STASH_LIST")?;
    Ok(())
}

/// Appends a newly-created stash entry as the new `stash@{0}`.
pub fn append_stash_entry(hash: &str, message: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crate::repo::path(STASH_LIST_PATH))
        .context("Failed to open .git/STASH_LIST")?;
    writeln!(file, "{} {}", hash, message)?;
    Ok(())
}

/// Parses a `stash@{N}` reference, a bare `N`, or `None` (meaning `stash@{0}`)
/// into an index into the stash list.
pub fn parse_stash_index(s: Option<&str>) -> anyhow::Result<usize> {
    match s {
        None => Ok(0),
        Some(raw) => {
            let trimmed = raw.trim();
            let inner = trimmed
                .strip_prefix("stash@{")
                .and_then(|r| r.strip_suffix('}'))
                .unwrap_or(trimmed);
            inner
                .parse::<usize>()
                .map_err(|_| anyhow::anyhow!("fatal: '{}' is not a valid stash reference", raw))
        }
    }
}

/// Human-readable label for the current HEAD, used in stash messages
/// ("WIP on <label>: ...").
pub fn current_branch_label() -> anyhow::Result<String> {
    match refs::resolve_head()? {
        refs::HeadState::Branch(b) => Ok(b),
        refs::HeadState::Detached(h) => {
            let short = &h[..h.len().min(7)];
            Ok(format!("(detached HEAD at {})", short))
        }
    }
}

/// Builds and writes a tree object directly from a flattened path -> (hash,
/// mode) map, reusing `write_tree_from_index_prefix` via a throwaway
/// `IndexEntry` list. Stat fields are irrelevant here since only the mode and
/// blob hash are encoded into tree entries.
pub fn build_tree_from_map(map: &BTreeMap<String, ([u8; 20], u32)>) -> anyhow::Result<String> {
    let entries: Vec<IndexEntry> = map
        .iter()
        .map(|(path, (hash, mode))| IndexEntry {
            ctime_secs: 0,
            ctime_nsecs: 0,
            mtime_secs: 0,
            mtime_nsecs: 0,
            dev: 0,
            ino: 0,
            mode: *mode,
            uid: 0,
            gid: 0,
            size: 0,
            hash: *hash,
            path: path.clone(),
            skip_worktree: false,
            stage: 0,
        })
        .collect();

    crate::helpers::objects::write_tree_from_index_prefix(&entries, "")
}

/// Given a stash index, reads its `w_commit`/`i_commit` pair and flattens
/// both into path -> (hash, mode) maps: `(working_tree, index_tree)`.
pub fn load_stash_trees(idx: usize, list: &[StashEntry]) -> anyhow::Result<(BTreeMap<String, ([u8; 20], u32)>, BTreeMap<String, ([u8; 20], u32)>)> {
    let w_hash = &list[idx].hash;

    let (obj_type, content) = read_object(w_hash)?;
    if obj_type != "commit" {
        anyhow::bail!("fatal: corrupted stash entry stash@{{{}}}", idx);
    }
    let text = String::from_utf8_lossy(&content).to_string();
    let parents = commit_parents(&text);
    if parents.len() < 2 {
        anyhow::bail!("fatal: corrupted stash entry stash@{{{}}}", idx);
    }
    let head_at_stash = &parents[0];
    let i_hash = &parents[1];

    let w_tree_hash = tree_hash_of_commit(w_hash)?;
    let mut w_tree = BTreeMap::new();
    flatten_tree(&w_tree_hash, "", &mut w_tree)?;

    let i_tree_hash = tree_hash_of_commit(i_hash)?;
    let mut i_tree = BTreeMap::new();
    flatten_tree(&i_tree_hash, "", &mut i_tree)?;

    let _ = head_at_stash; // only needed by `stash show`, kept here for symmetry
    Ok((w_tree, i_tree))
}

/// Shared implementation of `stash apply` and `stash pop`.
pub fn stash_apply_or_pop(stash_ref: Option<String>, drop_after: bool) -> anyhow::Result<()> {
    let idx = parse_stash_index(stash_ref.as_deref())?;
    let list = read_stash_list()?;
    if list.is_empty() {
        anyhow::bail!("No stash entries found.");
    }
    if idx >= list.len() {
        anyhow::bail!("fatal: stash@{{{}}} is not a valid reference", idx);
    }

    let action = if drop_after { "apply stash (pop)" } else { "apply stash" };
    ensure_working_tree_clean(action)?;

    let (w_tree, i_tree) = load_stash_trees(idx, &list)?;
    // A stash made with `-u`/`-a` keeps the untracked files in the `w` commit's third parent.
    if let Ok((_, content)) = read_object(&list[idx].hash) {
        if let Some(untracked) = commit_parents(&String::from_utf8_lossy(&content)).get(2) {
            restore_untracked_files(untracked)?;
        }
    }

    let head_tree: BTreeMap<String, ([u8; 20], u32)> = match refs::resolve_head_commit()? {
        Some(commit_hash) => {
            let tree_hash = tree_hash_of_commit(&commit_hash)?;
            let mut map = BTreeMap::new();
            flatten_tree(&tree_hash, "", &mut map)?;
            map
        }
        None => BTreeMap::new(),
    };

    // Remove tracked files that existed at HEAD but were deleted in the
    // stash's working-tree snapshot.
    for path in head_tree.keys() {
        if !w_tree.contains_key(path) {
            let p = Path::new(path);
            if p.exists() {
                let _ = fs::remove_file(p);
            }
        }
    }

    // Write the stash's working-tree snapshot to disk.
    for (path, (hash, _mode)) in &w_tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, &content).with_context(|| format!("failed to write '{}'", path))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = if (*_mode & 0o111) != 0 { 0o755 } else { 0o644 };
            fs::set_permissions(path, fs::Permissions::from_mode(file_mode))?;
        }
    }

    // Rebuild the index to match the stash's index snapshot (preserving the
    // staged vs. unstaged distinction that existed when the stash was made).
    let mut new_entries = build_index_entries_for_tree(&i_tree, true)?;
    write_index(&mut new_entries)?;

    let message = list[idx].message.clone();

    if drop_after {
        let mut remaining: Vec<StashEntry> = list;
        remaining.remove(idx);
        write_stash_list(&remaining)?;
        println!("Dropped stash@{{{}}} ({})", idx, message);
    } else {
        println!("Applied stash@{{{}}} ({})", idx, message);
    }

    Ok(())
}
/// Writes a stash commit object with any number of parents (the `w` commit has HEAD, the
/// index commit and, with `-u`/`-a`, the untracked-files commit; that one has none).
// Used by `stash push -u` / `-a`.
pub fn build_stash_commit(tree_hash: &str, parents: &[&str], message: &str) -> anyhow::Result<String> {
    let mut content = format!("tree {}\n", tree_hash);
    for parent in parents {
        content.push_str(&format!("parent {}\n", parent));
    }
    let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
    let author = crate::helpers::config::author_identity()?;
    content.push_str(&format!("author {} {} +0000\n", author, timestamp));
    content.push_str(&format!("committer {} {} +0000\n", author, timestamp));
    content.push_str(&format!("\n{}\n", message));
    crate::helpers::objects::write_object("commit", content.as_bytes())
}

/// Lists the untracked files of the working tree (`.gitignore`-aware; with `include_ignored`,
/// ignored files too), as repository-relative paths.
// Used by `stash push -u` / `-a`.
pub fn collect_untracked_files(tracked: &std::collections::BTreeSet<String>, include_ignored: bool) -> anyhow::Result<Vec<String>> {
    let mut tracked_dirs = std::collections::BTreeSet::new();
    for path in tracked {
        let mut prefix = String::new();
        let mut parts: Vec<&str> = path.split('/').collect();
        parts.pop();
        for part in parts {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            tracked_dirs.insert(prefix.clone());
        }
    }
    let mode = if include_ignored { crate::objects::CleanMode::IncludeIgnored } else { crate::objects::CleanMode::Normal };
    let (mut files, mut dirs, mut rules) = (Vec::new(), Vec::new(), Vec::new());
    crate::helpers::clean::collect_clean_candidates(Path::new("."), &mut rules, tracked, &tracked_dirs, &mode, false, &mut files, &mut dirs)?;
    files.sort();
    Ok(files)
}

/// Removes `path` and then every parent directory it leaves empty.
// Used by `stash push -u` / `-a` (after the untracked files are saved).
pub fn remove_file_and_empty_parents(path: &str) {
    let _ = fs::remove_file(path);
    let mut parent = Path::new(path).parent();
    while let Some(dir) = parent {
        if dir.as_os_str().is_empty() || fs::remove_dir(dir).is_err() {
            break;
        }
        parent = dir.parent();
    }
}

/// Restores the untracked files saved in a stash's third parent, refusing (before writing
/// anything) when one of them already exists, like git's "already exists, no checkout".
// Used by `stash apply` / `stash pop`.
fn restore_untracked_files(untracked_commit: &str) -> anyhow::Result<()> {
    let tree_hash = tree_hash_of_commit(untracked_commit)?;
    let mut tree = BTreeMap::new();
    flatten_tree(&tree_hash, "", &mut tree)?;
    let existing: Vec<&String> = tree.keys().filter(|p| Path::new(p.as_str()).exists()).collect();
    if !existing.is_empty() {
        for path in existing {
            eprintln!("{} already exists, no checkout", path);
        }
        eprintln!("error: could not restore untracked files from stash");
        std::process::exit(1);
    }
    for (path, (hash, mode)) in &tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, &content).with_context(|| format!("failed to write '{}'", path))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = if (*mode & 0o111) != 0 { 0o755 } else { 0o644 };
            fs::set_permissions(path, fs::Permissions::from_mode(file_mode))?;
        }
        let _ = mode;
    }
    Ok(())
}
