pub mod cli;
pub mod engine;
pub mod types;

use std::collections::{BTreeMap, BTreeSet};

use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit, write_object};
use crate::core::refs;
use crate::workspace::absorb::engine::{apply_hunks, describe, hunks_of, label, lines_of, place, rewritable_commits, would_conflict};
use crate::workspace::absorb::types::{Placement, StagedHunk};

type PathMap = BTreeMap<String, ([u8; 20], u32)>;

/// `rgit absorb [-n] [-f]`: splits the staged changes into hunks and folds each into the commit
/// that last changed the lines it replaces (blame), as long as that commit can be rewritten
/// (not pushed, no merge in between). One `fixup!` commit per target, all squashed in one
/// rewrite. Hunks it can't place (pure additions, lines from several commits, pushed commits,
/// new/deleted/binary files) stay staged; unstaged changes stay as they are.
pub fn absorb(dry_run: bool, force: bool) -> anyhow::Result<()> {
    crate::workspace::fixup::refuse_if_busy();
    let head_state = refs::resolve_head()?;
    let Some(head) = refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits to absorb into yet");
    };
    crate::merge::merge::engine::die_if_unmerged_for_commit()?;
    if !crate::workspace::fixup::engine::has_staged_changes(&head)? {
        crate::ui::die_fatal("nothing staged to absorb\nhint: stage the fixes with `rgit add <path>` (or `rgit add -p`)");
    }

    // HEAD vs index, file by file.
    let mut head_tree: PathMap = BTreeMap::new();
    flatten_tree(&tree_hash_of_commit(&head)?, "", &mut head_tree)?;
    let index: PathMap = crate::core::index::read_index().unwrap_or_default().iter().map(|e| (e.path.clone(), (e.hash, e.mode))).collect();
    let rewritable = rewritable_commits(&head, force)?;

    let mut head_lines: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut plan: Vec<(StagedHunk, Placement)> = Vec::new();
    let mut whole_files: Vec<(String, String)> = Vec::new();
    let paths: BTreeSet<&String> = head_tree.keys().chain(index.keys()).collect();
    for path in paths {
        let (old, new) = (head_tree.get(path), index.get(path));
        if old == new {
            continue;
        }
        let reason = match (old, new) {
            (None, _) => Some("new file (nothing to blame)"),
            (_, None) => Some("deleted file"),
            (Some((_, m1)), Some((_, m2))) if m1 != m2 => Some("mode change"),
            _ => None,
        };
        if let Some(reason) = reason {
            whole_files.push((path.clone(), reason.to_string()));
            continue;
        }
        let (old_blob, new_blob) = (read_object(&hex::encode(old.unwrap().0))?.1, read_object(&hex::encode(new.unwrap().0))?.1);
        if old_blob.contains(&0) || new_blob.contains(&0) {
            whole_files.push((path.clone(), "binary file".to_string()));
            continue;
        }
        let (old_lines, new_lines) = (lines_of(&old_blob), lines_of(&new_blob));
        let owners: Vec<String> = crate::history::blame::engine::blame_file(path, &head, false, None)?.into_iter().map(|b| b.commit).collect();
        for hunk in hunks_of(path, &old_lines, &new_lines) {
            let mut placement = if owners.len() == old_lines.len() { place(&hunk, &owners, &rewritable) } else { Placement::Left("couldn't blame this file".to_string()) };
            if let Placement::Into(target) = &placement
                && would_conflict(&hunk, &old_lines, target)?
            {
                placement = Placement::Left(format!("would conflict with a later change next to it (last changed in {})", &target[..7]));
            }
            plan.push((hunk, placement));
        }
        head_lines.insert(path.clone(), old_lines);
    }

    // The plan.
    let targets: Vec<String> = rewritable.iter().rev().filter(|c| plan.iter().any(|(_, p)| *p == Placement::Into((*c).clone()))).cloned().collect();
    let names = describe(&targets)?;
    let absorbed = plan.iter().filter(|(_, p)| matches!(p, Placement::Into(_))).count();
    let left = plan.len() - absorbed + whole_files.len();
    let width = plan.iter().map(|(h, _)| label(h).len()).chain(whole_files.iter().map(|(p, _)| p.len())).max().unwrap_or(0);
    if absorbed > 0 {
        let rewrites = rewritable.iter().position(|c| *c == targets[0]).map(|i| i + 1).unwrap_or(0);
        println!("Absorbing {} of {} staged hunk{} into {} (rewriting the last {}):", absorbed, absorbed + left, if absorbed + left == 1 { "" } else { "s" }, commits(targets.len()), commits(rewrites));
        for (hunk, placement) in &plan {
            if let Placement::Into(c) = placement {
                println!("  {:<width$}  -> {}", label(hunk), names[c], width = width);
            }
        }
    }
    if left > 0 {
        println!("{}", if absorbed > 0 { "Staying staged:" } else { "Nothing to absorb; everything stays staged:" });
        for (hunk, placement) in &plan {
            if let Placement::Left(why) = placement {
                println!("  {:<width$}  {}", label(hunk), why, width = width);
            }
        }
        for (path, why) in &whole_files {
            println!("  {:<width$}  {}", path, why, width = width);
        }
    }
    if dry_run {
        println!("(dry run: nothing changed)");
        return Ok(());
    }
    if absorbed == 0 {
        return Ok(());
    }

    // One fixup! commit per target, oldest target first, each adding that target's hunks on top
    // of the previous one (all hunks are against HEAD's lines, so a subset applies directly).
    let mut parent = head.clone();
    let mut fixups: Vec<(String, String)> = Vec::new();
    for (k, target) in targets.iter().enumerate() {
        let mut tree = head_tree.clone();
        for (path, old_lines) in &head_lines {
            let chosen: Vec<&StagedHunk> = plan
                .iter()
                .filter(|(h, p)| h.path == *path && matches!(p, Placement::Into(c) if targets[..=k].contains(c)))
                .map(|(h, _)| h)
                .collect();
            if chosen.is_empty() {
                continue;
            }
            let blob = write_object("blob", apply_hunks(old_lines, &chosen).as_bytes())?;
            let mut raw = [0u8; 20];
            hex::decode_to_slice(&blob, &mut raw)?;
            let mode = head_tree[path].1;
            tree.insert(path.clone(), (raw, mode));
        }
        let tree_hash = crate::workspace::state_files::build_tree_from_map(&tree)?;
        let subject = names[target].split_once(' ').map(|(_, s)| s).unwrap_or("");
        let fixup = crate::workspace::state_files::build_stash_commit(&tree_hash, &[&parent], &format!("fixup! {}", subject))?;
        fixups.push((target.clone(), fixup.clone()));
        parent = fixup;
    }
    let oldest_parent = parse_commit_object_parent(&targets[0])?;
    let mut items = Vec::new();
    for hash in crate::merge::rebase::engine::collect_commits_to_replay(&head, &oldest_parent)? {
        let subject = crate::merge::rebase::engine::subject_of(&hash)?;
        items.push(crate::merge::rebase::types::TodoItem { action: crate::merge::rebase::types::Action::Pick, hash: hash.clone(), subject });
        if let Some((_, fixup)) = fixups.iter().find(|(t, _)| *t == hash) {
            let subject = crate::merge::rebase::engine::subject_of(fixup)?;
            items.push(crate::merge::rebase::types::TodoItem { action: crate::merge::rebase::types::Action::Fixup, hash: fixup.clone(), subject });
        }
    }

    // Park the staged and unstaged changes in the stash during the rewrite (kept there if it
    // stops), then put them back exactly: the files as they were, the index as staged.
    let snap = crate::workspace::stash::capture_snapshot(None)?.expect("staged changes exist");
    let parked_message = format!("On {}: rgit absorb: your staged and unstaged changes", snap.branch_label);
    let parked = crate::workspace::state_files::build_stash_commit(&snap.working_tree, &[&snap.head, &snap.index_commit], &parked_message)?;
    crate::workspace::state_files::append_stash_entry(&parked, &parked_message)?;
    crate::workspace::stash::clear_saved_changes(&snap)?;

    crate::ui::note_on_failure(Some(
        "rgit absorb stopped on a conflict while folding the hunks in: resolve it, `rgit add` and `rgit rebase --continue`, then `rgit stash pop` (your staged and unstaged changes);\nor `rgit undo` to go back to before the absorb.".to_string(),
    ));
    crate::merge::rebase::engine::begin_replay(&head_state, &head, &oldest_parent, items, false)?;
    crate::ui::note_on_failure(None);

    restore_parked(&parked)?;
    println!("Absorbed {} into {}{}.", hunks(absorbed), commits(targets.len()), if left > 0 { format!("; {} still staged", hunks(left)) } else { String::new() });
    Ok(())
}

/// Puts the parked working tree and index back exactly as they were and drops the stash entry.
// Used by `absorb`.
fn restore_parked(parked: &str) -> anyhow::Result<()> {
    let (w_tree, i_tree) = crate::workspace::state_files::load_commit_trees(parked)?.ok_or_else(|| anyhow::anyhow!("fatal: lost the parked changes ({})", parked))?;
    let mut new_head: PathMap = BTreeMap::new();
    if let Some(h) = refs::resolve_head_commit()? {
        flatten_tree(&tree_hash_of_commit(&h)?, "", &mut new_head)?;
    }
    let mut touched: BTreeSet<String> = new_head.keys().cloned().collect();
    touched.extend(w_tree.keys().cloned());
    crate::workspace::checkout::hard_reset_working_tree(&w_tree, &touched)?;
    let mut entries = crate::workspace::checkout::build_index_entries_for_tree(&i_tree, true)?;
    crate::core::index::write_index(&mut entries)?;
    let mut list = crate::workspace::state_files::read_stash_list()?;
    list.retain(|e| e.hash != parked);
    crate::workspace::state_files::write_stash_list(&list)?;
    Ok(())
}

/// A commit's first parent.
// Used by `absorb`.
fn parse_commit_object_parent(hash: &str) -> anyhow::Result<String> {
    Ok(crate::core::commit::parse_commit_object(hash)?.parents[0].clone())
}

/// `1 hunk` / `3 hunks`.
// Used by `absorb`.
fn hunks(n: usize) -> String {
    format!("{} hunk{}", n, if n == 1 { "" } else { "s" })
}

/// `1 commit` / `3 commits`.
// Used by `absorb`.
fn commits(n: usize) -> String {
    crate::remote::sync::engine::commits(n)
}
