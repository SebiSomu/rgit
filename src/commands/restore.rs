use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::commands::commit::build_commit;
use crate::commands::plumbing::write_tree_from_index_prefix;
use crate::helpers::checkout::{check_switch_safety, ensure_working_tree_clean, sync_working_tree, update_index_from_tree};
use crate::helpers::state_files::read_lines_set;
use crate::helpers::{build_index_entries_for_tree, commit_parents, extract_commit_message, find_merge_base, generate_conflict_markers, hard_reset_working_tree, read_object, resolve_commit_from_source, resolve_tree_from_source, three_way_tree_merge, tree_hash_of_commit};
use crate::index::{build_entry, read_index, write_index};
use crate::refs;

const REBASE_TODO_PATH: &str = ".git/REBASE_TODO";
const REBASE_BRANCH_PATH: &str = ".git/REBASE_BRANCH";
const REBASE_ORIG_HEAD_PATH: &str = ".git/REBASE_ORIG_HEAD";

/// `rgit rebase <upstream>` / `--continue` / `--abort`
pub fn rebase(upstream: Option<String>, cont: bool, abort: bool) -> anyhow::Result<()> {
    if abort {
        return rebase_abort();
    }
    if cont {
        return rebase_continue();
    }

    let upstream = upstream.ok_or_else(|| {
        anyhow::anyhow!("fatal: rebase requires an <upstream>, or --continue / --abort")
    })?;

    if Path::new(REBASE_TODO_PATH).exists() || Path::new(REBASE_ORIG_HEAD_PATH).exists() {
        anyhow::bail!(
            "fatal: a rebase is already in progress\n\
             hint: use 'rgit rebase --continue' or 'rgit rebase --abort'"
        );
    }

    ensure_working_tree_clean("start a rebase")?;

    let head_state = refs::resolve_head()?;
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
    let upstream_commit = resolve_commit_from_source(&upstream)?;

    let merge_base = find_merge_base(&our_commit, &upstream_commit)?
        .ok_or_else(|| anyhow::anyhow!("fatal: refusing to rebase unrelated histories"))?;

    if merge_base == upstream_commit {
        println!("Current branch is up to date.");
        return Ok(());
    }

    if merge_base == our_commit {
        let target_tree = resolve_tree_from_source(&upstream_commit)?;
        let head_tree = resolve_tree_from_source(&our_commit)?;
        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> =
            index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

        check_switch_safety(&target_tree, &head_tree, &index_map)?;
        sync_working_tree(&target_tree, &head_tree, &index_map)?;
        update_index_from_tree(&target_tree, &head_tree)?;

        match &head_state {
            refs::HeadState::Branch(branch_name) => {
                let ref_path = format!("refs/heads/{}", branch_name);
                refs::write_ref(&ref_path, &upstream_commit)?;
            }
            refs::HeadState::Detached(_) => {
                refs::set_head_detached(&upstream_commit)?;
            }
        }

        println!("Fast-forwarded to {}.", &upstream_commit[..7]);
        return Ok(());
    }

    let commits_to_replay = collect_commits_to_replay(&our_commit, &merge_base)?;

    let branch_marker = match &head_state {
        refs::HeadState::Branch(branch_name) => format!("refs/heads/{}", branch_name),
        refs::HeadState::Detached(_) => "HEAD".to_string(),
    };
    fs::write(REBASE_BRANCH_PATH, format!("{}\n", branch_marker))
        .context("Failed to write .git/REBASE_BRANCH")?;
    fs::write(REBASE_ORIG_HEAD_PATH, format!("{}\n", our_commit))
        .context("Failed to write .git/REBASE_ORIG_HEAD")?;
    write_rebase_todo(&commits_to_replay)?;

    let target_tree = resolve_tree_from_source(&upstream_commit)?;
    let head_tree = resolve_tree_from_source(&our_commit)?;
    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, [u8; 20]> =
        index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

    check_switch_safety(&target_tree, &head_tree, &index_map)?;
    sync_working_tree(&target_tree, &head_tree, &index_map)?;
    update_index_from_tree(&target_tree, &head_tree)?;
    refs::set_head_detached(&upstream_commit)?;

    println!("Rebasing {} commit(s) onto {}...", commits_to_replay.len(), &upstream_commit[..7]);

    process_rebase_todo()
}

/// `rgit rebase --continue`
fn rebase_continue() -> anyhow::Result<()> {
    if !Path::new(REBASE_TODO_PATH).exists() {
        anyhow::bail!("fatal: no rebase in progress");
    }

    let todo = read_lines_set(REBASE_TODO_PATH)?;
    let Some((pick_commit, rest)) = todo.split_first() else {
        return finish_rebase();
    };

    let entries = read_index().unwrap_or_default();
    for entry in &entries {
        if let Ok(content) = fs::read(&entry.path) {
            if content.windows(7).any(|w| w == b"<<<<<<<") {
                anyhow::bail!(
                    "error: '{}' still has unresolved conflict markers; fix it and 'rgit add' it first",
                    entry.path
                );
            }
        }
    }

    let current_head = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;

    let (object_type, content) = read_object(pick_commit)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' does not point to a commit object", pick_commit);
    }
    let text = String::from_utf8_lossy(&content).to_string();
    let original_message = extract_commit_message(&text);
    let subject = original_message.lines().next().unwrap_or("").to_string();

    let tree_hash = write_tree_from_index_prefix(&entries, "")?;

    if tree_hash_of_commit(&current_head)? == tree_hash {
        println!("Skipping {}... {} (now empty)", &pick_commit[..7], subject);
    } else {
        let new_commit_hash = build_commit(tree_hash, Some(current_head), &original_message)?;
        refs::set_head_detached(&new_commit_hash)?;
        println!("[detached {}] {}", &new_commit_hash[..7], subject);
    }

    write_rebase_todo(rest)?;

    process_rebase_todo()
}

/// `rgit rebase --abort`
fn rebase_abort() -> anyhow::Result<()> {
    if !Path::new(REBASE_ORIG_HEAD_PATH).exists() {
        anyhow::bail!("fatal: no rebase in progress");
    }

    let orig_head = fs::read_to_string(REBASE_ORIG_HEAD_PATH)
        .context("Failed to read .git/REBASE_ORIG_HEAD")?
        .trim()
        .to_string();
    let branch_marker = fs::read_to_string(REBASE_BRANCH_PATH)
        .context("Failed to read .git/REBASE_BRANCH")?
        .trim()
        .to_string();

    let orig_tree = resolve_tree_from_source(&orig_head)?;

    let current_head = refs::resolve_head_commit()?;
    let current_tree = match &current_head {
        Some(h) => resolve_tree_from_source(h)?,
        None => BTreeMap::new(),
    };

    let index_entries = read_index().unwrap_or_default();
    let mut tracked_paths: std::collections::BTreeSet<String> =
        index_entries.iter().map(|e| e.path.clone()).collect();
    tracked_paths.extend(orig_tree.keys().cloned());
    tracked_paths.extend(current_tree.keys().cloned());

    hard_reset_working_tree(&orig_tree, &tracked_paths)?;
    let mut new_index_entries = build_index_entries_for_tree(&orig_tree, true)?;
    write_index(&mut new_index_entries)?;

    if branch_marker != "HEAD" {
        let branch_name = branch_marker.strip_prefix("refs/heads/").unwrap_or(&branch_marker).to_string();
        refs::set_head(&branch_name)?;
    } else {
        refs::set_head_detached(&orig_head)?;
    }

    cleanup_rebase_state();

    println!("Rebase aborted; HEAD left at {}.", &orig_head[..7]);

    Ok(())
}

fn process_rebase_todo() -> anyhow::Result<()> {
    loop {
        let todo = read_lines_set(REBASE_TODO_PATH)?;
        let Some((pick_commit, rest)) = todo.split_first() else {
            return finish_rebase();
        };
        let pick_commit = pick_commit.clone();

        let current_head = refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;

        let (object_type, content) = read_object(&pick_commit)?;
        if object_type != "commit" {
            anyhow::bail!("fatal: '{}' does not point to a commit object", pick_commit);
        }
        let text = String::from_utf8_lossy(&content).to_string();
        let parents = commit_parents(&text);
        if parents.len() > 1 {
            anyhow::bail!(
                "error: cannot rebase past merge commit {} (no support for replaying merges)",
                &pick_commit[..7]
            );
        }
        let original_message = extract_commit_message(&text);
        let subject = original_message.lines().next().unwrap_or("").to_string();

        let base_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
            resolve_tree_from_source(parent)?
        } else {
            BTreeMap::new()
        };
        let their_tree = resolve_tree_from_source(&pick_commit)?;
        let our_tree = resolve_tree_from_source(&current_head)?;

        let (merged_tree, conflicts) = three_way_tree_merge(&base_tree, &our_tree, &their_tree)?;

        if !conflicts.is_empty() {
            for conflict in &conflicts {
                let conflict_content = generate_conflict_markers(
                    &conflict.ours,
                    &conflict.theirs,
                    &format!("{}...", &pick_commit[..7]),
                );
                if let Some(parent) = Path::new(&conflict.path).parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)?;
                    }
                }
                fs::write(&conflict.path, &conflict_content)?;
                println!("CONFLICT (content): Merge conflict in {}", conflict.path);
            }

            for (path, (hash, _mode)) in &merged_tree {
                let (_, content) = read_object(&hex::encode(hash))?;
                if let Some(parent) = Path::new(path).parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)?;
                    }
                }
                fs::write(path, &content)?;
            }

            println!("error: could not apply {}... {}", &pick_commit[..7], subject);
            println!("hint: after resolving the conflicts, mark the corrected paths");
            println!("hint: with 'rgit add <paths>' and run 'rgit rebase --continue'");
            println!("hint: (or 'rgit rebase --abort' to give up)");
            return Ok(());
        }

        for path in our_tree.keys() {
            if !merged_tree.contains_key(path) {
                let path_obj = Path::new(path);
                if path_obj.exists() {
                    let _ = fs::remove_file(path_obj);
                }
            }
        }

        let mut new_index_entries = Vec::new();
        for (path, (hash, _mode)) in &merged_tree {
            let (_, content) = read_object(&hex::encode(hash))?;
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(path, &content)?;

            let metadata = fs::metadata(path)?;
            let entry = build_entry(path, *hash, &metadata);
            new_index_entries.push(entry);
        }
        write_index(&mut new_index_entries)?;

        let tree_hash = write_tree_from_index_prefix(&new_index_entries, "")?;

        if tree_hash_of_commit(&current_head)? == tree_hash {
            println!("Skipping {}... {} (now empty)", &pick_commit[..7], subject);
            write_rebase_todo(rest)?;
            continue;
        }

        let new_commit_hash = build_commit(tree_hash, Some(current_head.clone()), &original_message)?;
        refs::set_head_detached(&new_commit_hash)?;
        println!("[detached {}] {}", &new_commit_hash[..7], subject);

        write_rebase_todo(rest)?;
    }
}

fn finish_rebase() -> anyhow::Result<()> {
    let final_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;

    let branch_marker = fs::read_to_string(REBASE_BRANCH_PATH)
        .context("fatal: missing .git/REBASE_BRANCH for the in-progress rebase")?
        .trim()
        .to_string();

    if branch_marker != "HEAD" {
        let branch_name = branch_marker.strip_prefix("refs/heads/").unwrap_or(&branch_marker).to_string();
        refs::write_ref(&branch_marker, &final_commit)?;
        refs::set_head(&branch_name)?;
        println!("Successfully rebased and updated {}.", branch_marker);
    } else {
        println!("Successfully rebased onto {} (detached HEAD).", &final_commit[..7]);
    }

    cleanup_rebase_state();

    Ok(())
}

fn cleanup_rebase_state() {
    let _ = fs::remove_file(REBASE_TODO_PATH);
    let _ = fs::remove_file(REBASE_BRANCH_PATH);
    let _ = fs::remove_file(REBASE_ORIG_HEAD_PATH);
}

fn write_rebase_todo(remaining: &[String]) -> anyhow::Result<()> {
    if remaining.is_empty() {
        let _ = fs::remove_file(REBASE_TODO_PATH);
        return Ok(());
    }
    let mut content = remaining.join("\n");
    content.push('\n');
    fs::write(REBASE_TODO_PATH, content).context("Failed to write .git/REBASE_TODO")?;
    Ok(())
}

fn collect_commits_to_replay(tip: &str, base: &str) -> anyhow::Result<Vec<String>> {
    let mut commits = Vec::new();
    let mut current = tip.to_string();

    while current != base {
        let (object_type, content) = read_object(&current)?;
        if object_type != "commit" {
            anyhow::bail!("fatal: '{}' is not a commit object", current);
        }
        let text = String::from_utf8_lossy(&content).to_string();
        let parents = commit_parents(&text);

        if parents.len() > 1 {
            anyhow::bail!(
                "error: cannot rebase past merge commit {} (no support for replaying merges)",
                &current[..7]
            );
        }

        commits.push(current.clone());

        match parents.first() {
            Some(p) => current = p.clone(),
            None => anyhow::bail!(
                "fatal: reached root commit {} without finding the merge base — histories are unrelated",
                &current[..7]
            ),
        }
    }

    commits.reverse();
    Ok(commits)
}