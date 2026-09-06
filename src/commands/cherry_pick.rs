use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::helpers::{build_index_entries_for_tree, hard_reset_working_tree};
use crate::commands::plumbing::write_tree_from_index_prefix;
use crate::commands::commit::build_commit;
use crate::helpers::{commit_parents, extract_commit_message, generate_conflict_markers, read_object, resolve_commit_from_source, resolve_tree_from_source, three_way_tree_merge, tree_hash_of_commit};
use crate::index::{build_entry, read_index, write_index};
use crate::refs;

/// Replays a single commit's changes (relative to its own parent) onto the current
/// HEAD, similar to `git cherry-pick <commit>`. On a clean apply, creates a new
/// commit with a single parent (the current HEAD) carrying the original message
/// plus a `(cherry picked from commit ...)` trailer. On conflicts, writes conflict
/// markers into the working tree and records `.git/CHERRY_PICK_HEAD` /
/// `.git/CHERRY_PICK_MSG` so `--continue` or `--abort` can finish the job later.
// Used by the `cherry-pick` command.
pub fn cherry_pick(commit: Option<String>, no_commit: bool, cont: bool, abort: bool) -> anyhow::Result<()> {
    if abort {
        return cherry_pick_abort();
    }
    if cont {
        return cherry_pick_continue();
    }

    let commit_ref = commit.ok_or_else(|| {
        anyhow::anyhow!("fatal: cherry-pick requires a <commit>, or --continue / --abort")
    })?;

    if Path::new(".git/CHERRY_PICK_HEAD").exists() {
        anyhow::bail!(
            "fatal: a cherry-pick is already in progress\n\
             hint: use 'rgit cherry-pick --continue' or 'rgit cherry-pick --abort'"
        );
    }

    let head_state = refs::resolve_head()?;
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;

    let pick_commit = resolve_commit_from_source(&commit_ref)?;
    let (object_type, content) = read_object(&pick_commit)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' does not point to a commit object", commit_ref);
    }
    let text = String::from_utf8_lossy(&content).to_string();

    let parents = commit_parents(&text);
    if parents.len() > 1 {
        anyhow::bail!(
            "error: commit {} is a merge but no -m option was given.\nfatal: cherry-pick failed",
            &pick_commit[..7]
        );
    }

    let original_message = extract_commit_message(&text);
    let final_message = format!("{}\n\n(cherry picked from commit {})", original_message, pick_commit);
    let subject = original_message.lines().next().unwrap_or("").to_string();

    let base_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
        resolve_tree_from_source(parent)?
    } else {
        BTreeMap::new()
    };
    let their_tree = resolve_tree_from_source(&pick_commit)?;
    let our_tree = resolve_tree_from_source(&our_commit)?;

    let (merged_tree, conflicts) = three_way_tree_merge(&base_tree, &our_tree, &their_tree)?;

    if !conflicts.is_empty() {
        for conflict in &conflicts {
            let conflict_content =
                generate_conflict_markers(&conflict.ours, &conflict.theirs, &format!("{}...", &pick_commit[..7]));
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

        fs::write(".git/CHERRY_PICK_HEAD", format!("{}\n", pick_commit))?;
        fs::write(".git/CHERRY_PICK_MSG", format!("{}\n", final_message))?;

        println!("error: could not apply {}... {}", &pick_commit[..7], subject);
        println!("hint: after resolving the conflicts, mark the corrected paths");
        println!("hint: with 'rgit add <paths>' and run 'rgit cherry-pick --continue'");
        println!("hint: (or 'rgit cherry-pick --abort' to give up)");
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

    if tree_hash_of_commit(&our_commit)? == tree_hash {
        println!(
            "The previous cherry-pick is now empty, possibly due to conflict resolution."
        );
        println!("nothing to commit");
        return Ok(());
    }

    if no_commit {
        println!("Applied {}... {}", &pick_commit[..7], subject);
        println!("(staged, not committed -- run 'rgit commit -m \"...\"')");
        return Ok(());
    }

    let commit_hash = build_commit(tree_hash, Some(our_commit.clone()), &final_message)?;
    let short_hash = &commit_hash[..7];

    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            let ref_path = format!("refs/heads/{}", branch_name);
            refs::write_ref(&ref_path, &commit_hash)?;
            println!("[{} {}] {}", branch_name, short_hash, subject);
        }
        refs::HeadState::Detached(_) => {
            refs::set_head_detached(&commit_hash)?;
            println!("[(detached HEAD) {}] {}", short_hash, subject);
        }
    }

    Ok(())
}

/// Finishes an in-progress cherry-pick after conflicts have been resolved and
/// staged with `add`. Refuses if unresolved `<<<<<<<` markers remain anywhere in
/// the index's files, then commits with the original message + trailer that was
/// saved to `.git/CHERRY_PICK_MSG` when the conflict was first hit.
// Used by `cherry-pick --continue`.
fn cherry_pick_continue() -> anyhow::Result<()> {
    if !Path::new(".git/CHERRY_PICK_HEAD").exists() {
        anyhow::bail!("fatal: no cherry-pick in progress");
    }

    let pick_commit = fs::read_to_string(".git/CHERRY_PICK_HEAD")?.trim().to_string();
    let final_message = fs::read_to_string(".git/CHERRY_PICK_MSG")
        .context("fatal: missing .git/CHERRY_PICK_MSG for the in-progress cherry-pick")?
        .trim_end_matches('\n')
        .to_string();

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

    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;

    let tree_hash = write_tree_from_index_prefix(&entries, "")?;
    let commit_hash = build_commit(tree_hash, Some(our_commit), &final_message)?;
    let short_hash = &commit_hash[..7];

    let head_state = refs::resolve_head()?;
    match &head_state {
        refs::HeadState::Branch(branch_name) => {
            let ref_path = format!("refs/heads/{}", branch_name);
            refs::write_ref(&ref_path, &commit_hash)?;
        }
        refs::HeadState::Detached(_) => {
            refs::set_head_detached(&commit_hash)?;
        }
    }

    let _ = fs::remove_file(".git/CHERRY_PICK_HEAD");
    let _ = fs::remove_file(".git/CHERRY_PICK_MSG");

    println!(
        "[{} {}] cherry-pick of {} continued",
        match &head_state {
            refs::HeadState::Branch(b) => b.clone(),
            refs::HeadState::Detached(_) => "(detached HEAD)".to_string(),
        },
        short_hash,
        &pick_commit[..7]
    );

    Ok(())
}

/// Cancels an in-progress cherry-pick, restoring the working tree and index to
/// exactly match HEAD (which never moved during a conflicted pick) and discarding
/// any partially-applied changes or conflict-marker files it left behind.
// Used by `cherry-pick --abort`.
fn cherry_pick_abort() -> anyhow::Result<()> {
    if !Path::new(".git/CHERRY_PICK_HEAD").exists() {
        anyhow::bail!("fatal: no cherry-pick in progress");
    }

    let pick_commit = fs::read_to_string(".git/CHERRY_PICK_HEAD")?.trim().to_string();
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
    let our_tree = resolve_tree_from_source(&our_commit)?;

    let (object_type, content) = read_object(&pick_commit)?;
    let (merged_tree, conflicts) = if object_type == "commit" {
        let text = String::from_utf8_lossy(&content).to_string();
        let parents = commit_parents(&text);
        let base_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
            resolve_tree_from_source(parent)?
        } else {
            BTreeMap::new()
        };
        let their_tree = resolve_tree_from_source(&pick_commit)?;
        three_way_tree_merge(&base_tree, &our_tree, &their_tree)?
    } else {
        (BTreeMap::new(), Vec::new())
    };

    let index_entries = read_index().unwrap_or_default();
    let mut tracked_paths: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();
    tracked_paths.extend(our_tree.keys().cloned());
    tracked_paths.extend(merged_tree.keys().cloned());
    tracked_paths.extend(conflicts.iter().map(|c| c.path.clone()));

    hard_reset_working_tree(&our_tree, &tracked_paths)?;

    let mut new_index_entries = build_index_entries_for_tree(&our_tree, true)?;
    write_index(&mut new_index_entries)?;

    let _ = fs::remove_file(".git/CHERRY_PICK_HEAD");
    let _ = fs::remove_file(".git/CHERRY_PICK_MSG");

    println!("Cherry-pick of {} aborted; HEAD left unchanged.", &pick_commit[..7]);

    Ok(())
}