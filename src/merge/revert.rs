pub mod cli;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use anyhow::Context;
use crate::workspace::checkout::{build_index_entries_for_tree, hard_reset_working_tree};
use crate::workspace::state_files::build_tree_from_map;
use crate::core::commit::{
    build_commit,
    commit_parents,
    commit_subject_line,
    resolve_commit_from_source,
    resolve_tree_from_source,
};
use crate::core::object_store::{read_object, tree_hash_of_commit, write_tree_from_index_prefix};
use crate::merge::merge::engine::{die_if_unmerged, die_if_unmerged_for_commit, merge_into_worktree};
use crate::core::index::{read_index, write_index};
use crate::core::refs;

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

    die_if_unmerged("Reverting", "revert failed")?;
    if crate::core::repo::path(".git/REVERT_HEAD").exists() {
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

    let their_label = format!("parent of {} ({})", &revert_commit[..7], original_subject);
    let result = match merge_into_worktree(&base_tree, &our_tree, &their_tree, "HEAD", &their_label, "merge") {
        Ok(result) => result,
        Err(e) => {
            eprintln!("{}", e);
            eprintln!("fatal: revert failed");
            crate::ui::exit(128);
        }
    };
    for message in &result.messages {
        println!("{}", message.text);
    }

    if !result.conflicted.is_empty() {
        fs::write(crate::core::repo::path(".git/REVERT_HEAD"), format!("{}\n", revert_commit))?;
        fs::write(crate::core::repo::path(".git/REVERT_MSG"), format!("{}\n", final_message))?;

        eprintln!("error: could not revert {}... {}", &revert_commit[..7], original_subject);
        // The narration's conflict report replaces these on a terminal.
        if !crate::ui::narration_replaces_hints() {
            crate::ui::eprintln_hints("hint: After resolving the conflicts, mark them with");
            crate::ui::eprintln_hints("hint: \"rgit add/rm <pathspec>\", then run");
            crate::ui::eprintln_hints("hint: \"rgit revert --continue\".");
            crate::ui::eprintln_hints("hint: To abort and get back to the state before \"rgit revert\",");
            crate::ui::eprintln_hints("hint: run \"rgit revert --abort\".");
        }
        crate::ui::exit(1);
    }

    let tree_hash = build_tree_from_map(&result.tree)?;

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
    if !crate::core::repo::path(".git/REVERT_HEAD").exists() {
        eprintln!("error: no cherry-pick or revert in progress\nfatal: revert failed");
        crate::ui::exit(128);
    }

    let revert_commit = fs::read_to_string(crate::core::repo::path(".git/REVERT_HEAD"))?.trim().to_string();
    let final_message = fs::read_to_string(crate::core::repo::path(".git/REVERT_MSG"))
        .context("fatal: missing .git/REVERT_MSG for the in-progress revert")?
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

    let _ = fs::remove_file(crate::core::repo::path(".git/REVERT_HEAD"));
    let _ = fs::remove_file(crate::core::repo::path(".git/REVERT_MSG"));

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
    if !crate::core::repo::path(".git/REVERT_HEAD").exists() {
        eprintln!("error: no cherry-pick or revert in progress\nfatal: revert failed");
        crate::ui::exit(128);
    }

    let revert_commit = fs::read_to_string(crate::core::repo::path(".git/REVERT_HEAD"))?.trim().to_string();
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
    let our_tree = resolve_tree_from_source(&our_commit)?;

    // Every path the revert touched is in the index (conflicted ones as stages 1-3).
    let index_entries = read_index().unwrap_or_default();
    let mut tracked_paths: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();
    tracked_paths.extend(our_tree.keys().cloned());

    hard_reset_working_tree(&our_tree, &tracked_paths)?;

    let mut new_index_entries = build_index_entries_for_tree(&our_tree, true)?;
    write_index(&mut new_index_entries)?;

    let _ = fs::remove_file(crate::core::repo::path(".git/REVERT_HEAD"));
    let _ = fs::remove_file(crate::core::repo::path(".git/REVERT_MSG"));

    println!("Revert of {} aborted; HEAD left unchanged.", &revert_commit[..7]);

    Ok(())
}