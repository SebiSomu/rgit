pub mod types;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use anyhow::Context;

use crate::workspace::checkout::{build_index_entries_for_tree, ensure_working_tree_clean};
use crate::core::commit::commit_parents;
use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit};
use crate::core::index::{write_index, IndexEntry};

use crate::core::refs;
use crate::workspace::state_files::types::StashEntry;

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
    if !crate::core::repo::path(STASH_LIST_PATH).exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(crate::core::repo::path(STASH_LIST_PATH)).context("Failed to read .git/STASH_LIST")?;
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
        if crate::core::repo::path(STASH_LIST_PATH).exists() {
            fs::remove_file(crate::core::repo::path(STASH_LIST_PATH)).context("Failed to remove .git/STASH_LIST")?;
        }
        return Ok(());
    }

    let mut lines: Vec<String> = entries.iter().map(|e| format!("{} {}", e.hash, e.message)).collect();
    lines.reverse();
    let mut content = lines.join("\n");
    content.push('\n');
    fs::write(crate::core::repo::path(STASH_LIST_PATH), content).context("Failed to write .git/STASH_LIST")?;
    Ok(())
}

/// Appends a newly-created stash entry as the new `stash@{0}`.
pub fn append_stash_entry(hash: &str, message: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(crate::core::repo::path(STASH_LIST_PATH))
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

    crate::core::object_store::write_tree_from_index_prefix(&entries, "")
}

/// A flattened tree: path -> (blob hash, mode).
type PathMap = BTreeMap<String, ([u8; 20], u32)>;

/// Given a stash index, reads its `w_commit`/`i_commit` pair and flattens
/// both into path -> (hash, mode) maps: `(working_tree, index_tree)`.
pub fn load_stash_trees(idx: usize, list: &[StashEntry]) -> anyhow::Result<(BTreeMap<String, ([u8; 20], u32)>, BTreeMap<String, ([u8; 20], u32)>)> {
    load_commit_trees(&list[idx].hash)?.ok_or_else(|| anyhow::anyhow!("fatal: corrupted stash entry stash@{{{}}}", idx))
}

/// Reads a stash-shaped commit (`w`: tree = working tree, second parent = index commit) and
/// flattens both trees into path -> (hash, mode) maps: `(working_tree, index_tree)`. None when
/// the object is not such a commit.
// Used by `load_stash_trees` and `rgit wip --restore`.
pub(crate) fn load_commit_trees(w_hash: &str) -> anyhow::Result<Option<(PathMap, PathMap)>> {
    let (obj_type, content) = read_object(w_hash)?;
    if obj_type != "commit" {
        return Ok(None);
    }
    let parents = commit_parents(&String::from_utf8_lossy(&content));
    if parents.len() < 2 {
        return Ok(None);
    }

    let w_tree_hash = tree_hash_of_commit(w_hash)?;
    let mut w_tree = BTreeMap::new();
    flatten_tree(&w_tree_hash, "", &mut w_tree)?;

    let i_tree_hash = tree_hash_of_commit(&parents[1])?;
    let mut i_tree = BTreeMap::new();
    flatten_tree(&i_tree_hash, "", &mut i_tree)?;

    Ok(Some((w_tree, i_tree)))
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
    crate::ui::narrate::clear_stash_conflict();
    if !apply_stash_trees(&list[idx].hash, &w_tree, &i_tree, "Stashed changes", "apply stash")? {
        let _ = fs::write(crate::core::repo::path(crate::ui::narrate::STASH_CONFLICT_PATH), format!("stash@{{{}}}\n", idx));
        if drop_after {
            eprintln!("The stash entry is kept in case you need it again.");
        }
        crate::ui::exit(1);
    }

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

/// Puts a stash-shaped commit's changes back: its untracked files (third parent) first, then,
/// when HEAD is still the commit it was made on, its working tree and index exactly; otherwise a
/// three-way merge onto HEAD (base = HEAD at save time; markers "Updated upstream" /
/// `theirs_label`). After a clean merge the changes are left unstaged and new files staged.
/// Returns false when the merge left conflicts (cleanly merged files staged, conflicted ones at
/// stages 1-3, messages printed). The caller has checked the working tree is clean.
// Used by `stash apply` / `stash pop` and `rgit wip --restore`.
pub(crate) fn apply_stash_trees(w_hash: &str, w_tree: &BTreeMap<String, ([u8; 20], u32)>, i_tree: &BTreeMap<String, ([u8; 20], u32)>, theirs_label: &str, action: &str) -> anyhow::Result<bool> {
    let stash_parents = read_object(w_hash).map(|(_, c)| commit_parents(&String::from_utf8_lossy(&c))).unwrap_or_default();
    // A stash made with `-u`/`-a` keeps the untracked files in the `w` commit's third parent.
    if let Some(untracked) = stash_parents.get(2) {
        restore_untracked_files(untracked)?;
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

    // HEAD moved since the stash was made: three-way merge the stashed changes onto it
    // (base = HEAD at stash time), like git, instead of overwriting the newer commits.
    let head_now = refs::resolve_head_commit()?;
    if let Some(base) = stash_parents.first().filter(|b| head_now.as_deref() != Some(b.as_str())) {
        let base_tree = crate::core::commit::resolve_tree_from_source(base)?;
        let result = crate::merge::merge::engine::merge_into_worktree(&base_tree, &head_tree, w_tree, "Updated upstream", theirs_label, action)?;
        for m in &result.messages {
            println!("{}", m.text);
        }
        if !result.conflicted.is_empty() {
            return Ok(false);
        }
        // Clean: back to HEAD in the index, except files the stash adds, which stay staged.
        let mut staged = head_tree.clone();
        for (path, entry) in &result.tree {
            if !head_tree.contains_key(path) {
                staged.insert(path.clone(), *entry);
            }
        }
        let mut entries = build_index_entries_for_tree(&staged, true)?;
        write_index(&mut entries)?;
        return Ok(true);
    }

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
    for (path, (hash, _mode)) in w_tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, crate::core::eol::smudge(path, &content)).with_context(|| format!("failed to write '{}'", path))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = if (*_mode & 0o111) != 0 { 0o755 } else { 0o644 };
            fs::set_permissions(path, fs::Permissions::from_mode(file_mode))?;
        }
    }

    // Rebuild the index to match the stash's index snapshot (preserving the
    // staged vs. unstaged distinction that existed when the stash was made).
    let mut new_entries = build_index_entries_for_tree(i_tree, true)?;
    write_index(&mut new_entries)?;
    Ok(true)
}

/// Writes a stash commit object with any number of parents (the `w` commit has HEAD, the
/// index commit and, with `-u`/`-a`, the untracked-files commit; that one has none).
// Used by `stash push -u` / `-a`.
pub fn build_stash_commit(tree_hash: &str, parents: &[&str], message: &str) -> anyhow::Result<String> {
    let parents: Vec<String> = parents.iter().map(|p| p.to_string()).collect();
    crate::core::commit::write_commit_now(tree_hash, &parents, message)
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
    let mode = if include_ignored { crate::workspace::clean::types::CleanMode::IncludeIgnored } else { crate::workspace::clean::types::CleanMode::Normal };
    let (mut files, mut dirs, mut rules) = (Vec::new(), Vec::new(), crate::workspace::gitignore::base_exclude_rules());
    crate::workspace::clean::engine::collect_clean_candidates(Path::new("."), &mut rules, tracked, &tracked_dirs, &mode, false, &mut files, &mut dirs)?;
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
        crate::ui::exit(1);
    }
    for (path, (hash, mode)) in &tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, crate::core::eol::smudge(path, &content)).with_context(|| format!("failed to write '{}'", path))?;
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

