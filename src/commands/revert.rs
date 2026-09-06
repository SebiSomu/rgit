
// ============================================================================
// revert
// ============================================================================
//
// A revert is a cherry-pick run backward: instead of replaying a commit's
// changes forward onto HEAD, it applies the *inverse* of those changes.
// Concretely, the same three-way merge cherry-pick uses is reused here with
// the roles swapped:
//
//   cherry-pick <C>: base = parent(C), ours = HEAD, theirs = C
//   revert      <C>: base = C,         ours = HEAD, theirs = parent(C)
//
// i.e. "merge in the parent's tree, treating the commit itself as the
// common ancestor" — which is exactly the diff that undoes C.
//
// Conflict state mirrors cherry-pick's `.git/CHERRY_PICK_HEAD` /
// `.git/CHERRY_PICK_MSG` via `.git/REVERT_HEAD` / `.git/REVERT_MSG`, and
// `--continue`/`--abort` work the same way.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::helpers::{build_index_entries_for_tree, commit_subject_line, hard_reset_working_tree};
use crate::commands::commit::build_commit;
use crate::commands::plumbing::write_tree_from_index_prefix;
use crate::helpers::{commit_parents, generate_conflict_markers, read_object, resolve_commit_from_source, resolve_tree_from_source, three_way_tree_merge, tree_hash_of_commit};
use crate::index::{build_entry, read_index, write_index};
use crate::refs;

/// `rgit revert <commit>` / `--continue` / `--abort`
pub fn revert(commit: Option<String>, no_commit: bool, cont: bool, abort: bool) -> anyhow::Result<()> {
    if abort {
        return revert_abort();
    }
    if cont {
        return revert_continue();
    }

    let commit_ref = commit.ok_or_else(|| {
        anyhow::anyhow!("fatal: revert requires a <commit>, or --continue / --abort")
    })?;

    if Path::new(".git/REVERT_HEAD").exists() {
        anyhow::bail!(
            "fatal: a revert is already in progress\n\
             hint: use 'rgit revert --continue' or 'rgit revert --abort'"
        );
    }

    let head_state = refs::resolve_head()?;
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;

    let revert_commit = resolve_commit_from_source(&commit_ref)?;
    let (object_type, content) = read_object(&revert_commit)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' does not point to a commit object", commit_ref);
    }
    let text = String::from_utf8_lossy(&content).to_string();

    let parents = commit_parents(&text);
    if parents.len() > 1 {
        anyhow::bail!(
            "error: commit {} is a merge but no -m option was given.\nfatal: revert failed",
            &revert_commit[..7]
        );
    }

    let original_subject = commit_subject_line(&revert_commit)?;
    let subject = format!("Revert \"{}\"", original_subject);
    let final_message = format!("{}\n\nThis reverts commit {}.", subject, revert_commit);

    let base_tree = resolve_tree_from_source(&revert_commit)?;
    let their_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
        resolve_tree_from_source(parent)?
    } else {
        BTreeMap::new()
    };
    let our_tree = resolve_tree_from_source(&our_commit)?;

    let (merged_tree, conflicts) = three_way_tree_merge(&base_tree, &our_tree, &their_tree)?;

    if !conflicts.is_empty() {
        for conflict in &conflicts {
            let conflict_content = generate_conflict_markers(
                &conflict.ours,
                &conflict.theirs,
                &format!("parent of {}...", &revert_commit[..7]),
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

        fs::write(".git/REVERT_HEAD", format!("{}\n", revert_commit))?;
        fs::write(".git/REVERT_MSG", format!("{}\n", final_message))?;

        println!("error: could not revert {}... {}", &revert_commit[..7], original_subject);
        println!("hint: after resolving the conflicts, mark the corrected paths");
        println!("hint: with 'rgit add <paths>' and run 'rgit revert --continue'");
        println!("hint: (or 'rgit revert --abort' to give up)");
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
        println!("The revert is now empty, possibly due to conflict resolution.");
        println!("nothing to commit");
        return Ok(());
    }

    if no_commit {
        println!("Reverted {}... {}", &revert_commit[..7], original_subject);
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

/// `rgit revert --continue`
fn revert_continue() -> anyhow::Result<()> {
    if !Path::new(".git/REVERT_HEAD").exists() {
        anyhow::bail!("fatal: no revert in progress");
    }

    let revert_commit = fs::read_to_string(".git/REVERT_HEAD")?.trim().to_string();
    let final_message = fs::read_to_string(".git/REVERT_MSG")
        .context("fatal: missing .git/REVERT_MSG for the in-progress revert")?
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

    let _ = fs::remove_file(".git/REVERT_HEAD");
    let _ = fs::remove_file(".git/REVERT_MSG");

    println!(
        "[{} {}] revert of {} continued",
        match &head_state {
            refs::HeadState::Branch(b) => b.clone(),
            refs::HeadState::Detached(_) => "(detached HEAD)".to_string(),
        },
        short_hash,
        &revert_commit[..7]
    );

    Ok(())
}

/// `rgit revert --abort`
fn revert_abort() -> anyhow::Result<()> {
    if !Path::new(".git/REVERT_HEAD").exists() {
        anyhow::bail!("fatal: no revert in progress");
    }

    let revert_commit = fs::read_to_string(".git/REVERT_HEAD")?.trim().to_string();
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
    let our_tree = resolve_tree_from_source(&our_commit)?;

    let (object_type, content) = read_object(&revert_commit)?;
    let (merged_tree, conflicts) = if object_type == "commit" {
        let text = String::from_utf8_lossy(&content).to_string();
        let parents = commit_parents(&text);
        let base_tree = resolve_tree_from_source(&revert_commit)?;
        let their_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
            resolve_tree_from_source(parent)?
        } else {
            BTreeMap::new()
        };
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

    let _ = fs::remove_file(".git/REVERT_HEAD");
    let _ = fs::remove_file(".git/REVERT_MSG");

    println!("Revert of {} aborted; HEAD left unchanged.", &revert_commit[..7]);

    Ok(())
}