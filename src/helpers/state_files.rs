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

pub fn read_lines_set(path: &str) -> anyhow::Result<Vec<String>> {
    if !Path::new(path).exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).with_context(|| format!("Failed to read {}", path))?;
    Ok(content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

pub fn append_line(path: &str, line: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("Failed to open {}", path))?;
    writeln!(file, "{}", line)?;
    Ok(())
}

/// Reads all stash entries, most-recent-first (`stash@{0}` == `list[0]`).
pub fn read_stash_list() -> anyhow::Result<Vec<StashEntry>> {
    if !Path::new(STASH_LIST_PATH).exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(STASH_LIST_PATH).context("Failed to read .git/STASH_LIST")?;
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
        if Path::new(STASH_LIST_PATH).exists() {
            fs::remove_file(STASH_LIST_PATH).context("Failed to remove .git/STASH_LIST")?;
        }
        return Ok(());
    }

    let mut lines: Vec<String> = entries.iter().map(|e| format!("{} {}", e.hash, e.message)).collect();
    lines.reverse();
    let mut content = lines.join("\n");
    content.push('\n');
    fs::write(STASH_LIST_PATH, content).context("Failed to write .git/STASH_LIST")?;
    Ok(())
}

/// Appends a newly-created stash entry as the new `stash@{0}`.
pub fn append_stash_entry(hash: &str, message: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(STASH_LIST_PATH)
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
        })
        .collect();

    crate::commands::plumbing::write_tree_from_index_prefix(&entries, "")
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