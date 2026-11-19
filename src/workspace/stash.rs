pub mod types;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use anyhow::Context;

use crate::workspace::checkout::{build_index_entries_for_tree, hard_reset_working_tree};
use crate::workspace::state_files::{
    append_stash_entry,
    build_tree_from_map,
    current_branch_label,
    parse_stash_index,
    read_stash_list,
    stash_apply_or_pop,
    STASH_LIST_PATH,
    write_stash_list,
};
use crate::merge::merge::engine::build_merge_commit;
use crate::diff::myers::format_diff_output;
use crate::core::commit::{
    build_commit,
    commit_parents,
    commit_subject_line,
    resolve_tree_from_source,
};
use crate::core::object_store::{
    flatten_tree,
    hash_content,
    read_object,
    tree_hash_of_commit,
    write_object,
    write_tree_from_index_prefix,
};
use crate::core::index::{read_index, write_index};
use crate::core::refs;
use crate::workspace::stash::types::StashAction;

/// `rgit stash` / `rgit stash push [-m <message>]`
///
/// Snapshots the current index and (tracked) working directory into a pair
/// of commit objects, records the stash entry, then resets the working
/// directory and index back to exactly match HEAD. With `untracked` (`-u`: Some(false),
/// `-a`: Some(true), ignored files included) the untracked files go into a third,
/// parentless commit and are removed from the working tree, like git.
pub fn stash_push(message: Option<String>, untracked: Option<bool>) -> anyhow::Result<()> {
    let head_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: You do not have the initial commit yet"))?;

    let head_tree_hash = tree_hash_of_commit(&head_commit)?;
    let mut head_tree = BTreeMap::new();
    flatten_tree(&head_tree_hash, "", &mut head_tree)?;

    let index_entries = read_index().unwrap_or_default();
    let index_tree_hash = write_tree_from_index_prefix(&index_entries, "")?;

    let mut working_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
    for entry in &index_entries {
        let path_obj = Path::new(&entry.path);
        if !path_obj.exists() {
            continue;
        }

        let content = fs::read(path_obj).with_context(|| format!("Failed to read {}", entry.path))?;
        let hash = hash_content("blob", &content);
        if hash != entry.hash {
            write_object("blob", &content)?;
        }

        #[cfg(unix)]
        let mode: u32 = {
            use std::os::unix::fs::PermissionsExt;
            let perm = fs::metadata(path_obj)?.permissions().mode();
            if perm & 0o111 != 0 { 0o100755 } else { 0o100644 }
        };
        #[cfg(not(unix))]
        let mode: u32 = entry.mode;

        working_tree.insert(entry.path.clone(), (hash, mode));
    }

    let working_tree_hash = build_tree_from_map(&working_tree)?;

    let tracked_now: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();
    let untracked_files = match untracked {
        Some(include_ignored) => crate::workspace::state_files::collect_untracked_files(&tracked_now, include_ignored)?,
        None => Vec::new(),
    };

    if index_tree_hash == head_tree_hash && working_tree_hash == head_tree_hash && untracked_files.is_empty() {
        println!("No local changes to save");
        return Ok(());
    }

    let branch_label = current_branch_label()?;
    let short_head = &head_commit[..7];
    let subject = commit_subject_line(&head_commit)?;
    let index_message = format!("index on {}: {} {}", branch_label, short_head, subject);
    let i_commit_hash = build_commit(index_tree_hash, Some(head_commit.clone()), &index_message)?;

    let stash_message = match &message {
        Some(m) => format!("On {}: {}", branch_label, m),
        None => format!("WIP on {}: {} {}", branch_label, short_head, subject),
    };
    let w_commit_hash = if untracked_files.is_empty() {
        build_merge_commit(working_tree_hash, &head_commit, &i_commit_hash, &stash_message)?
    } else {
        let mut untracked_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
        for path in &untracked_files {
            let content = fs::read(path).with_context(|| format!("Failed to read {}", path))?;
            let hash = write_object("blob", &content)?;
            let mut raw = [0u8; 20];
            hex::decode_to_slice(&hash, &mut raw)?;
            #[cfg(unix)]
            let mode: u32 = {
                use std::os::unix::fs::PermissionsExt;
                if fs::metadata(path)?.permissions().mode() & 0o111 != 0 { 0o100755 } else { 0o100644 }
            };
            #[cfg(not(unix))]
            let mode: u32 = 0o100644;
            untracked_tree.insert(path.clone(), (raw, mode));
        }
        let u_tree = build_tree_from_map(&untracked_tree)?;
        let u_message = format!("untracked files on {}: {} {}", branch_label, short_head, subject);
        let u_commit = crate::workspace::state_files::build_stash_commit(&u_tree, &[], &u_message)?;
        crate::workspace::state_files::build_stash_commit(&working_tree_hash, &[&head_commit, &i_commit_hash, &u_commit], &stash_message)?
    };

    append_stash_entry(&w_commit_hash, &stash_message)?;

    let mut tracked_paths: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();
    tracked_paths.extend(head_tree.keys().cloned());
    tracked_paths.extend(working_tree.keys().cloned());

    hard_reset_working_tree(&head_tree, &tracked_paths)?;
    let mut new_entries = build_index_entries_for_tree(&head_tree, true)?;
    write_index(&mut new_entries)?;
    for path in &untracked_files {
        crate::workspace::state_files::remove_file_and_empty_parents(path);
    }

    println!("Saved working directory and index state {}", stash_message);

    Ok(())
}

/// `rgit stash pop [<stash>]`
pub fn stash_pop(stash_ref: Option<String>) -> anyhow::Result<()> {
    stash_apply_or_pop(stash_ref, true)
}

/// `rgit stash apply [<stash>]`
pub fn stash_apply(stash_ref: Option<String>) -> anyhow::Result<()> {
    stash_apply_or_pop(stash_ref, false)
}

/// `rgit stash list`
pub fn stash_list() -> anyhow::Result<()> {
    let list = read_stash_list()?;
    for (i, entry) in list.iter().enumerate() {
        println!("stash@{{{}}}: {}", i, entry.message);
    }
    Ok(())
}

/// `rgit stash drop [<stash>]`
pub fn stash_drop(stash_ref: Option<String>) -> anyhow::Result<()> {
    let idx = parse_stash_index(stash_ref.as_deref())?;
    let mut list = read_stash_list()?;
    if list.is_empty() {
        anyhow::bail!("No stash entries found.");
    }
    if idx >= list.len() {
        anyhow::bail!("fatal: stash@{{{}}} is not a valid reference", idx);
    }

    let removed = list.remove(idx);
    write_stash_list(&list)?;
    println!("Dropped stash@{{{}}} ({})", idx, removed.message);
    Ok(())
}

/// `rgit stash show [<stash>]`
///
/// Prints the diff between the HEAD the stash was taken against and its
/// working-tree snapshot.
pub fn stash_show(stash_ref: Option<String>) -> anyhow::Result<()> {
    let idx = parse_stash_index(stash_ref.as_deref())?;
    let list = read_stash_list()?;
    if list.is_empty() {
        anyhow::bail!("No stash entries found.");
    }
    if idx >= list.len() {
        anyhow::bail!("fatal: stash@{{{}}} is not a valid reference", idx);
    }

    let w_hash = &list[idx].hash;
    let (obj_type, content) = read_object(w_hash)?;
    if obj_type != "commit" {
        anyhow::bail!("fatal: corrupted stash entry stash@{{{}}}", idx);
    }
    let text = String::from_utf8_lossy(&content).to_string();
    let parents = commit_parents(&text);
    let head_at_stash = parents
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("fatal: corrupted stash entry stash@{{{}}}", idx))?;

    let base_tree = resolve_tree_from_source(&head_at_stash)?;
    let w_tree = resolve_tree_from_source(w_hash)?;

    let mut all_paths = BTreeSet::new();
    for p in base_tree.keys() {
        all_paths.insert(p.clone());
    }
    for p in w_tree.keys() {
        all_paths.insert(p.clone());
    }

    for path in all_paths {
        let old_bytes = if let Some((hash, _)) = base_tree.get(&path) {
            read_object(&hex::encode(hash))?.1
        } else {
            Vec::new()
        };
        let new_bytes = if let Some((hash, _)) = w_tree.get(&path) {
            read_object(&hex::encode(hash))?.1
        } else {
            Vec::new()
        };

        if old_bytes == new_bytes {
            continue;
        }

        let old_str = String::from_utf8_lossy(&old_bytes);
        let new_str = String::from_utf8_lossy(&new_bytes);
        let old_lines: Vec<&str> = if old_bytes.is_empty() { Vec::new() } else { old_str.lines().collect() };
        let new_lines: Vec<&str> = if new_bytes.is_empty() { Vec::new() } else { new_str.lines().collect() };

        let old_label = format!("a/{}", path);
        let new_label = format!("b/{}", path);
        let formatted = format_diff_output(&path, &old_lines, &new_lines, &old_label, &new_label);
        print!("{}", formatted);
    }

    Ok(())
}

/// `rgit stash clear`
pub fn stash_clear() -> anyhow::Result<()> {
    if crate::core::repo::path(STASH_LIST_PATH).exists() {
        fs::remove_file(crate::core::repo::path(STASH_LIST_PATH)).context("Failed to remove .git/STASH_LIST")?;
    }
    Ok(())
}

/// Top-level dispatcher for `rgit stash [<action>]`. A bare `rgit stash`
/// (no subcommand) behaves like `rgit stash push`.
pub fn stash(action: Option<crate::workspace::stash::types::StashAction>) -> anyhow::Result<()> {

    // Saving a stash needs a fully merged index: git just lists what needs merging.
    if matches!(action, None | Some(StashAction::Push { .. })) && crate::merge::merge::engine::report_needs_merge() {
        crate::ui::exit(1);
    }
    match action {
        None => stash_push(None, None),
        Some(StashAction::Push { message, patch: true, paths, .. }) => crate::workspace::patch::stash_patch(message, paths),
        Some(StashAction::Push { message, include_untracked, all, .. }) => {
            let untracked = if all { Some(true) } else if include_untracked { Some(false) } else { None };
            stash_push(message, untracked)
        }
        Some(StashAction::Pop { stash }) => stash_pop(stash),
        Some(StashAction::Apply { stash }) => stash_apply(stash),
        Some(StashAction::List) => stash_list(),
        Some(StashAction::Drop { stash }) => stash_drop(stash),
        Some(StashAction::Show { stash }) => stash_show(stash),
        Some(StashAction::Clear) => stash_clear(),
    }
}

