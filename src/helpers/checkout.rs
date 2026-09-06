use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::helpers::gitignore::{collect_files, normalize_path};
use crate::helpers::objects::{flatten_tree, hash_content, read_object, tree_hash_of_commit};
use crate::index::{build_entry, read_index, write_index, IndexEntry};
use crate::refs;

/// Checks whether switching branches would overwrite untracked or modified files in the working directory.
// Used by `switch` and `checkout` commands to prevent data loss.
pub fn check_switch_safety(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>, index_map: &BTreeMap<String, [u8; 20]>) -> anyhow::Result<()> {
    let mut working_files = Vec::new();
    collect_files(Path::new("."), &mut working_files)?;
    let mut working_map = BTreeMap::new();
    for file_path in &working_files {
        let rel_path = normalize_path(file_path);
        let content = fs::read(file_path)?;
        working_map.insert(rel_path, hash_content("blob", &content));
    }

    let mut local_changes = std::collections::BTreeSet::new();

    for (path, work_hash) in &working_map {
        match index_map.get(path) {
            None => {
                if let Some((target_hash, _)) = target_tree.get(path) {
                    if target_hash != work_hash {
                        anyhow::bail!("error: The following untracked working tree files would be overwritten by switch:\n\t{}\nPlease move or remove them before you switch branches.", path);
                    }
                }
            }
            Some(idx_hash) => {
                if idx_hash != work_hash {
                    local_changes.insert(path.clone());
                }
            }
        }
    }
    for path in index_map.keys() {
        if !working_map.contains_key(path) {
            local_changes.insert(path.clone());
        }
    }

    for (path, idx_hash) in index_map {
        match head_tree.get(path) {
            None => {
                local_changes.insert(path.clone());
            }
            Some((head_hash, _)) => {
                if head_hash != idx_hash {
                    local_changes.insert(path.clone());
                }
            }
        }
    }
    for path in head_tree.keys() {
        if !index_map.contains_key(path) {
            local_changes.insert(path.clone());
        }
    }

    let mut overwritten_files: Vec<String> = Vec::new();
    for path in &local_changes {
        let file_changes_in_switch = match (head_tree.get(path), target_tree.get(path)) {
            (Some((h, _)), Some((t, _))) => h != t,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (None, None) => false,
        };

        if !file_changes_in_switch {
            continue;
        }
        overwritten_files.push(path.clone());
    }

    if !overwritten_files.is_empty() {
        let mut msg = String::from("error: Your local changes to the following files would be overwritten by switch:\n");
        for file in overwritten_files {
            msg.push_str(&format!("\t{}\n", file));
        }
        msg.push_str("Please commit your changes or stash them before you switch branches.\nAborting");
        anyhow::bail!(msg);
    }

    Ok(())
}

/// Synchronizes the files in the working directory to match the target commit tree.
/// Writes modified/new files and cleans up files removed in the target.
// Used by `switch` and `checkout` commands.
pub fn sync_working_tree(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>, index_map: &BTreeMap<String, [u8; 20]>) -> anyhow::Result<()> {
    let mut files_to_delete = std::collections::BTreeSet::new();
    for path in index_map.keys() {
        if !target_tree.contains_key(path) {
            files_to_delete.insert(path.clone());
        }
    }
    for path in head_tree.keys() {
        if !target_tree.contains_key(path) {
            files_to_delete.insert(path.clone());
        }
    }

    for path in files_to_delete {
        let path_obj = Path::new(&path);
        if path_obj.exists() {
            fs::remove_file(path_obj)?;
            let mut parent = path_obj.parent();
            while let Some(p) = parent {
                if p == Path::new("") || p == Path::new(".") {
                    break;
                }
                if p.exists() {
                    if fs::read_dir(p)?.next().is_none() {
                        fs::remove_dir(p)?;
                    } else {
                        break;
                    }
                }
                parent = p.parent();
            }
        }
    }

    for (path, (hash, _mode)) in target_tree {
        let should_write = match head_tree.get(path) {
            None => true,
            Some((head_hash, _)) => head_hash != hash,
        };

        if should_write {
            let (_, content) = read_object(&hex::encode(hash))?;
            if let Some(parent) = Path::new(&path).parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, &content)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let file_mode = if (_mode & 0o111) != 0 { 0o755 } else { 0o644 };
                fs::set_permissions(path, fs::Permissions::from_mode(file_mode))?;
            }
        }
    }

    Ok(())
}

/// Updates the index entries to reflect target branch files after a switch.
// Used by `switch` and `checkout` commands.
pub fn update_index_from_tree(target_tree: &BTreeMap<String, ([u8; 20], u32)>, head_tree: &BTreeMap<String, ([u8; 20], u32)>) -> anyhow::Result<()> {
    let mut entries = read_index().unwrap_or_default();
    entries.retain(|e| target_tree.contains_key(&e.path));

    for (path, (hash, _)) in target_tree {
        let should_update = match head_tree.get(path) {
            None => true,
            Some((head_hash, _)) => head_hash != hash,
        };

        if should_update {
            let metadata = fs::metadata(path)?;
            let new_entry = build_entry(path, *hash, &metadata);
            entries.retain(|e| &e.path != path);
            entries.push(new_entry);
        }
    }

    write_index(&mut entries)?;
    Ok(())
}

/// Refuses to proceed unless the working directory and index are currently
/// clean (i.e. exactly match HEAD). `stash apply`/`pop` use this in place of
/// a real three-way merge back into a dirty tree.
pub fn ensure_working_tree_clean(action: &str) -> anyhow::Result<()> {
    let head_tree: BTreeMap<String, ([u8; 20], u32)> = match refs::resolve_head_commit()? {
        Some(commit_hash) => {
            let tree_hash = tree_hash_of_commit(&commit_hash)?;
            let mut map = BTreeMap::new();
            flatten_tree(&tree_hash, "", &mut map)?;
            map
        }
        None => BTreeMap::new(),
    };

    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, ([u8; 20], u32)> = index_entries
        .iter()
        .map(|e| (e.path.clone(), (e.hash, e.mode)))
        .collect();

    if index_map != head_tree {
        anyhow::bail!(
            "error: cannot {}: you have staged changes.\nPlease commit or reset them first.",
            action
        );
    }

    for (path, (head_hash, _mode)) in &head_tree {
        let path_obj = Path::new(path);
        if !path_obj.exists() {
            anyhow::bail!(
                "error: local file '{}' is missing; cannot safely {}.\nAborting",
                path,
                action
            );
        }

        let content = fs::read(path_obj).with_context(|| format!("Failed to read {}", path))?;
        let hash = hash_content("blob", &content);
        if hash != *head_hash {
            anyhow::bail!(
                "error: Your local changes to the following files would be overwritten by {}:\n\t{}\nPlease commit your changes or stash them before running this command again.\nAborting",
                action,
                path
            );
        }
    }

    Ok(())
}

/// Force-overwrites the working directory to exactly match `target_tree`, discarding
/// any uncommitted changes. Deletes files that were tracked (per `tracked_paths`) but
/// are absent from the target.
///
/// Unlike `sync_working_tree` (used by `switch`/`checkout`), every file present in the
/// target is rewritten unconditionally rather than only when it differs from the old
/// HEAD tree — `reset --hard` must discard local modifications even when their content
/// happens to match the previous HEAD.
// Used by `reset --hard`.
pub fn hard_reset_working_tree(target_tree: &BTreeMap<String, ([u8; 20], u32)>, tracked_paths: &std::collections::BTreeSet<String>, ) -> anyhow::Result<()> {
    for path in tracked_paths {
        if target_tree.contains_key(path) {
            continue;
        }

        let path_obj = Path::new(path);
        if path_obj.exists() {
            fs::remove_file(path_obj)
                .with_context(|| format!("failed to remove file '{}'", path))?;

            let mut parent = path_obj.parent();
            while let Some(p) = parent {
                if p == Path::new("") || p == Path::new(".") {
                    break;
                }
                if p.exists() {
                    if fs::read_dir(p)?.next().is_none() {
                        fs::remove_dir(p)?;
                    } else {
                        break;
                    }
                }
                parent = p.parent();
            }
        }
    }

    for (path, (hash, _mode)) in target_tree {
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

    Ok(())
}

/// Builds a fresh set of index entries describing exactly `target_tree`, independent of
/// whatever the index previously contained. When `read_disk_metadata` is true (after a
/// hard reset has just written every file to disk), real stat info is captured via
/// `build_entry`; otherwise (a mixed reset, which never touches the working tree) a
/// placeholder stat entry is used, mirroring the convention already used by `restore`.
// Used by `reset --mixed` and `reset --hard`.
pub fn build_index_entries_for_tree(target_tree: &BTreeMap<String, ([u8; 20], u32)>, read_disk_metadata: bool) -> anyhow::Result<Vec<IndexEntry>> {
    let mut entries = Vec::with_capacity(target_tree.len());

    for (path, (hash, mode)) in target_tree {
        if read_disk_metadata {
            if let Ok(metadata) = fs::metadata(path) {
                let mut entry = build_entry(path, *hash, &metadata);
                entry.mode = *mode;
                entries.push(entry);
                continue;
            }
        }

        let (_, content) = read_object(&hex::encode(hash))?;
        entries.push(IndexEntry {
            ctime_secs: 0,
            ctime_nsecs: 0,
            mtime_secs: 0,
            mtime_nsecs: 0,
            dev: 0,
            ino: 0,
            mode: *mode,
            uid: 0,
            gid: 0,
            size: content.len() as u32,
            hash: *hash,
            path: path.clone(),
        });
    }

    Ok(entries)
}