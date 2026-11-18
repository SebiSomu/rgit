use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use anyhow::Context;
use crate::helpers::{
    build_commit, build_index_entries_for_tree, build_tree_from_map, commit_parents, extract_commit_message,
    hard_reset_working_tree, read_object, resolve_commit_from_source, resolve_tree_from_source,
    tree_hash_of_commit, write_tree_from_index_prefix,
};
use crate::helpers::merge::{die_if_unmerged, die_if_unmerged_for_commit, merge_into_worktree};
use crate::index::{read_index, write_index};
use crate::refs;

/// Replays a single commit's changes (relative to its own parent) onto the current
/// HEAD, similar to `git cherry-pick <commit>`. On a clean apply, creates a new
/// commit with a single parent (the current HEAD) carrying the original message
/// plus a `(cherry picked from commit ...)` trailer. The merge is git's line-level three-way
/// merge with `HEAD` and `<abbrev> (<subject>)` as labels. On conflicts, the markers go into
/// the working tree, the conflicted paths get their stages 1-3 in the index, and
/// `.git/CHERRY_PICK_HEAD` / `.git/CHERRY_PICK_MSG` are recorded so `--continue` or `--abort`
/// can finish the job later.
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

    die_if_unmerged("Cherry-picking", "cherry-pick failed")?;
    if crate::repo::path(".git/CHERRY_PICK_HEAD").exists() {
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

    let their_label = format!("{} ({})", &pick_commit[..7], subject);
    let result = match merge_into_worktree(&base_tree, &our_tree, &their_tree, "HEAD", &their_label, "merge") {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{}", e);
            eprintln!("fatal: cherry-pick failed");
            std::process::exit(128);
        }
    };
    for message in &result.messages {
        println!("{}", message.text);
    }

    if !result.conflicted.is_empty() {
        fs::write(crate::repo::path(".git/CHERRY_PICK_HEAD"), format!("{}\n", pick_commit))?;
        fs::write(crate::repo::path(".git/CHERRY_PICK_MSG"), format!("{}\n", final_message))?;

        eprintln!("error: could not apply {}... {}", &pick_commit[..7], subject);
        eprintln!("hint: After resolving the conflicts, mark them with");
        eprintln!("hint: \"rgit add/rm <pathspec>\", then run");
        eprintln!("hint: \"rgit cherry-pick --continue\".");
        eprintln!("hint: To abort and get back to the state before \"rgit cherry-pick\",");
        eprintln!("hint: run \"rgit cherry-pick --abort\".");
        std::process::exit(1);
    }

    let tree_hash = build_tree_from_map(&result.tree)?;

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
/// staged with `add`. Refuses while the index still has unmerged paths, then commits with the original message + trailer that was
/// saved to `.git/CHERRY_PICK_MSG` when the conflict was first hit.
// Used by `cherry-pick --continue`.
fn cherry_pick_continue() -> anyhow::Result<()> {
    if !crate::repo::path(".git/CHERRY_PICK_HEAD").exists() {
        eprintln!("error: no cherry-pick or revert in progress\nfatal: cherry-pick failed");
        std::process::exit(128);
    }

    let pick_commit = fs::read_to_string(crate::repo::path(".git/CHERRY_PICK_HEAD"))?.trim().to_string();
    let final_message = fs::read_to_string(crate::repo::path(".git/CHERRY_PICK_MSG"))
        .context("fatal: missing .git/CHERRY_PICK_MSG for the in-progress cherry-pick")?
        .trim_end_matches('\n')
        .to_string();

    die_if_unmerged_for_commit()?;
    let entries = read_index().unwrap_or_default();

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

    let _ = fs::remove_file(crate::repo::path(".git/CHERRY_PICK_HEAD"));
    let _ = fs::remove_file(crate::repo::path(".git/CHERRY_PICK_MSG"));

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
    if !crate::repo::path(".git/CHERRY_PICK_HEAD").exists() {
        eprintln!("error: no cherry-pick or revert in progress\nfatal: cherry-pick failed");
        std::process::exit(128);
    }

    let pick_commit = fs::read_to_string(crate::repo::path(".git/CHERRY_PICK_HEAD"))?.trim().to_string();
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
    let our_tree = resolve_tree_from_source(&our_commit)?;

    // Every path the pick touched is in the index (conflicted ones as stages 1-3).
    let index_entries = read_index().unwrap_or_default();
    let mut tracked_paths: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();
    tracked_paths.extend(our_tree.keys().cloned());

    hard_reset_working_tree(&our_tree, &tracked_paths)?;

    let mut new_index_entries = build_index_entries_for_tree(&our_tree, true)?;
    write_index(&mut new_index_entries)?;

    let _ = fs::remove_file(crate::repo::path(".git/CHERRY_PICK_HEAD"));
    let _ = fs::remove_file(crate::repo::path(".git/CHERRY_PICK_MSG"));

    println!("Cherry-pick of {} aborted; HEAD left unchanged.", &pick_commit[..7]);

    Ok(())
}