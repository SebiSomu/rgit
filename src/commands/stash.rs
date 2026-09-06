use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use anyhow::Context;

use crate::commands::commit::build_commit;
use crate::commands::plumbing::write_tree_from_index_prefix;
use crate::helpers::{
    append_stash_entry, build_index_entries_for_tree, build_merge_commit, build_tree_from_map,
    commit_parents, commit_subject_line, current_branch_label, flatten_tree, format_diff_output,
    hard_reset_working_tree, hash_content, parse_stash_index, read_object, read_stash_list,
    resolve_tree_from_source, stash_apply_or_pop, tree_hash_of_commit, write_object, write_stash_list,
    STASH_LIST_PATH,
};
use crate::index::{read_index, write_index};
use crate::refs;

/// `rgit stash` / `rgit stash push [-m <message>]`
///
/// Snapshots the current index and (tracked) working directory into a pair
/// of commit objects, records the stash entry, then resets the working
/// directory and index back to exactly match HEAD.
pub fn stash_push(message: Option<String>) -> anyhow::Result<()> {
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

    if index_tree_hash == head_tree_hash && working_tree_hash == head_tree_hash {
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
    let w_commit_hash = build_merge_commit(working_tree_hash, &head_commit, &i_commit_hash, &stash_message)?;

    append_stash_entry(&w_commit_hash, &stash_message)?;

    let mut tracked_paths: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();
    tracked_paths.extend(head_tree.keys().cloned());
    tracked_paths.extend(working_tree.keys().cloned());

    hard_reset_working_tree(&head_tree, &tracked_paths)?;
    let mut new_entries = build_index_entries_for_tree(&head_tree, true)?;
    write_index(&mut new_entries)?;

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
    if Path::new(STASH_LIST_PATH).exists() {
        fs::remove_file(STASH_LIST_PATH).context("Failed to remove .git/STASH_LIST")?;
    }
    Ok(())
}

/// Top-level dispatcher for `rgit stash [<action>]`. A bare `rgit stash`
/// (no subcommand) behaves like `rgit stash push`.
pub fn stash(action: Option<crate::objects::StashAction>) -> anyhow::Result<()> {
    use crate::objects::StashAction;

    match action {
        None => stash_push(None),
        Some(StashAction::Push { message }) => stash_push(message),
        Some(StashAction::Pop { stash }) => stash_pop(stash),
        Some(StashAction::Apply { stash }) => stash_apply(stash),
        Some(StashAction::List) => stash_list(),
        Some(StashAction::Drop { stash }) => stash_drop(stash),
        Some(StashAction::Show { stash }) => stash_show(stash),
        Some(StashAction::Clear) => stash_clear(),
    }
}