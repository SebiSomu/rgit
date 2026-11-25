pub mod cli;
pub mod engine;
pub mod types;

use std::collections::BTreeMap;

use crate::core::object_store::{flatten_tree, tree_hash_of_commit};
use crate::core::refs;
use crate::merge::move_commit::engine::{replay_in_memory, select_commits};
use crate::merge::move_commit::types::Replayed;

/// `rgit move-commit <commit>... <branch> [-c] [--copy] [-f]`: cherry-picks commits of the
/// current branch onto another branch and drops them from the current one, without switching
/// branches. Both replays happen in memory first; when either would conflict, nothing is
/// changed. The other branch only has its ref moved; the current branch is rewritten from the
/// oldest moved commit (the commits on top are replayed without the moved ones) and the working
/// tree follows like a switch, keeping uncommitted changes (refused if they'd be overwritten).
/// `-c` creates the branch at the oldest moved commit's parent; `--copy` keeps the commits on
/// the current branch; dropping commits a remote-tracking branch has needs `-f`.
pub fn move_commit(specs: Vec<String>, branch: String, create: bool, copy: bool, force: bool) -> anyhow::Result<()> {
    crate::workspace::fixup::refuse_if_busy();
    let head_state = refs::resolve_head()?;
    let Some(head) = refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits to move yet");
    };
    let current = match &head_state {
        refs::HeadState::Branch(b) => b.clone(),
        refs::HeadState::Detached(_) => "HEAD".to_string(),
    };
    if branch == current {
        crate::ui::die_fatal(format!("'{}' is the current branch; name the branch to move the commits to", branch));
    }
    let (moved, chain) = match select_commits(&specs, &head) {
        Ok(found) => found,
        Err(e) => crate::ui::die_fatal(e.to_string()),
    };
    let base = crate::core::commit::parse_commit_object(&chain[0])?.parents[0].clone();
    let what = |n: usize| crate::remote::sync::engine::commits(n);

    let target_ref = format!("refs/heads/{}", branch);
    let existing = refs::read_ref(&target_ref)?;
    let target_tip = match existing.clone() {
        Some(tip) => tip,
        None if create => {
            if !refs::is_valid_branch_name(&branch) {
                crate::ui::die_fatal(format!("'{}' is not a valid branch name", branch));
            }
            base.clone()
        }
        None => crate::ui::die_fatal(format!("no branch named '{}'\nhint: pass -c to create it (at {}, where the oldest commit moved was made)", branch, &base[..7])),
    };
    if !copy && !force {
        for hash in &moved {
            if let Some(remote) = crate::workspace::fixup::engine::pushed_on(hash)? {
                crate::ui::die_fatal(format!(
                    "{} is already pushed ({} has it); dropping it would make the branches diverge\nhint: pass --force to drop it anyway (you'll need `rgit push --force` afterwards), or --copy to only copy it",
                    &hash[..7],
                    remote
                ));
            }
        }
    }
    let subject = |h: &str| crate::ui::narrate::commit_info(h).map(|(s, _, _)| s).unwrap_or_default();

    // 1. The commits on the other branch, in memory.
    let new_target = match replay_in_memory(&moved, &target_tip)? {
        Replayed::Done(tip) => tip,
        Replayed::Conflict { commit, paths } => crate::ui::die_fatal(format!(
            "{} {} doesn't apply cleanly on '{}' (conflicts in {}); nothing was changed\nhint: cherry-pick it by hand to resolve them: `rgit switch {}` then `rgit cherry-pick {}`",
            &commit[..7], subject(&commit), branch, paths.join(", "), branch, &commit[..7]
        )),
    };

    // 2. The current branch without them, in memory.
    let kept: Vec<String> = chain.iter().filter(|c| !moved.contains(c)).cloned().collect();
    let new_head = if copy {
        None
    } else {
        match replay_in_memory(&kept, &base)? {
            Replayed::Done(tip) => Some(tip),
            Replayed::Conflict { commit, paths } => crate::ui::die_fatal(format!(
                "{} {} needs the commits being moved (without them it conflicts in {}); nothing was changed\nhint: move it too, or pass --copy to only copy the commits to '{}'",
                &commit[..7], subject(&commit), paths.join(", "), branch
            )),
        }
    };

    // 3. The working tree and index follow the new HEAD (only files that differ change);
    // checked before any ref moves.
    if let Some(new_head) = &new_head {
        let flat = |c: &str| -> anyhow::Result<BTreeMap<String, ([u8; 20], u32)>> {
            let mut tree = BTreeMap::new();
            flatten_tree(&tree_hash_of_commit(c)?, "", &mut tree)?;
            Ok(tree)
        };
        let (old_tree, new_tree) = (flat(&head)?, flat(new_head)?);
        let index_map: BTreeMap<String, [u8; 20]> = crate::core::index::read_index().unwrap_or_default().iter().map(|e| (e.path.clone(), e.hash)).collect();
        let mut changed: Vec<String> = old_tree.keys().chain(new_tree.keys()).filter(|p| old_tree.get(*p) != new_tree.get(*p)).cloned().collect();
        changed.sort();
        changed.dedup();
        let dirty: std::collections::BTreeSet<String> = crate::core::commit::tracked_worktree_changes()?.into_iter().map(|(_, p)| p).collect();
        let in_the_way: Vec<&String> = changed
            .iter()
            .filter(|p| dirty.contains(*p) || index_map.get(*p) != old_tree.get(*p).map(|e| &e.0) || (!old_tree.contains_key(*p) && std::path::Path::new(p.as_str()).exists()))
            .collect();
        if !in_the_way.is_empty() {
            crate::ui::die_fatal(format!(
                "dropping the commits changes {}, which you have uncommitted changes in (or an untracked file at); nothing was changed\nhint: commit or `rgit wip` them first",
                in_the_way.iter().map(|p| p.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
        if let Err(e) = crate::workspace::checkout::check_switch_safety(&new_tree, &old_tree, &index_map) {
            crate::ui::die_fatal(format!("{}\nnothing was changed", e.to_string().trim_start_matches("error: ")));
        }
        refs::write_ref(&target_ref, &new_target)?;
        crate::workspace::checkout::sync_working_tree(&new_tree, &old_tree, &index_map)?;
        crate::workspace::checkout::update_index_from_tree(&new_tree, &old_tree)?;
        match &head_state {
            refs::HeadState::Branch(b) => refs::write_ref(&format!("refs/heads/{}", b), new_head)?,
            refs::HeadState::Detached(_) => refs::set_head_detached(new_head)?,
        }
    } else {
        refs::write_ref(&target_ref, &new_target)?;
    }

    let created = if existing.is_none() { format!(", a new branch off {}", &base[..7]) } else { String::new() };
    println!("{} {} to '{}' (now {}{}):", if copy { "Copied" } else { "Moved" }, what(moved.len()), branch, &new_target[..7], created);
    for hash in &moved {
        println!("  {} {}", &hash[..7], subject(hash));
    }
    if let Some(new_head) = &new_head {
        let replayed = match kept.len() {
            0 => String::new(),
            1 => ", replaying the commit on top".to_string(),
            n => format!(", replaying the {} on top", what(n)),
        };
        println!("Dropped {} from '{}' (now {}){}.", if moved.len() == 1 { "it" } else { "them" }, current, &new_head[..7], replayed);
    }
    Ok(())
}
