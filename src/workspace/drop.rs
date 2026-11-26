pub mod cli;

use anyhow::Result;

use crate::core::refs;
use crate::merge::rebase::types::{Action, TodoItem};

/// `rgit drop <commit>`: removes one commit from the current branch and replays the commits
/// after it onto its parent, with the rebase engine: a conflict stops like a rebase does
/// (`rebase --continue`, `--skip`, `--abort`), with the usual narration. Uncommitted changes
/// are stashed around the replay and applied again after it, as git's `rebase --autostash`
/// does (so staged changes come back unstaged). Refuses the root commit, merges
/// (dropping one or replaying past one), and pushed commits unless `force`.
// Used by `rgit drop`.
pub fn drop(commit: &str, force: bool) -> Result<()> {
    crate::workspace::fixup::refuse_if_busy();
    crate::merge::merge::engine::die_if_unmerged_for_commit()?;
    let head_state = refs::resolve_head()?;
    let Some(head) = refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits to drop yet");
    };
    let target = match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", commit)) {
        Ok(hash) => hash,
        Err(_) => crate::ui::die_fatal(format!("'{}' is not a commit", commit)),
    };
    let short = target[..7].to_string();
    let dropped = crate::core::commit::parse_commit_object(&target)?;
    let subject = dropped.message.lines().next().unwrap_or("").to_string();
    if let Err(e) = crate::workspace::reword::engine::reword_chain(&head, &target) {
        crate::ui::die_fatal(e.to_string().trim_start_matches("fatal: ").replace("reworded", "dropped"));
    }
    let parent = match dropped.parents.as_slice() {
        [parent] => parent.clone(),
        [] => crate::ui::die_fatal(format!("{} \"{}\" is the root commit; there is nothing to put the later commits on", short, subject)),
        _ => crate::ui::die_fatal(format!("{} \"{}\" is a merge commit; rgit can't drop a merge", short, subject)),
    };
    if !force && let Some(remote) = crate::workspace::fixup::engine::pushed_on(&target)? {
        crate::ui::die_fatal(format!(
            "{} \"{}\" is already pushed ({} has it); dropping it would make the branches diverge\nhint: pass --force to drop it anyway (you'll need `rgit push --force` afterwards)",
            short, subject, remote
        ));
    }
    let later = match crate::merge::rebase::engine::collect_commits_to_replay(&head, &target) {
        Ok(mut commits) => {
            commits.reverse();
            commits
        }
        Err(_) => crate::ui::die_fatal(format!("a merge commit sits between {} and HEAD; rgit can't replay merges", short)),
    };
    let mut items = Vec::with_capacity(later.len());
    for hash in later {
        let subject = crate::merge::rebase::engine::subject_of(&hash)?;
        items.push(TodoItem { action: Action::Pick, hash, subject });
    }
    let replayed = items.len();

    // Uncommitted changes would block the replay: park them in the stash meanwhile.
    let stashed = !crate::core::commit::tracked_worktree_changes()?.is_empty() || crate::workspace::fixup::engine::has_staged_changes(&head)?;
    if stashed {
        crate::workspace::stash::stash_push(Some("rgit drop: uncommitted changes".to_string()), None)?;
    }
    crate::ui::note_on_failure(Some(format!(
        "rgit drop stopped while replaying the commits after {} without it: resolve the conflicts, `rgit add` them and `rgit rebase --continue`{};\nor `rgit rebase --abort` (or `rgit undo`) to keep {} after all.",
        short,
        if stashed { ", then `rgit stash pop` (your uncommitted changes)" } else { "" },
        short
    )));
    crate::merge::rebase::engine::begin_replay(&head_state, &head, &parent, items, false)?;
    crate::ui::note_on_failure(None);

    let new_head = refs::resolve_head_commit()?.unwrap_or_default();
    println!(
        "Dropped {} \"{}\"{}; HEAD is now {}.",
        short,
        subject,
        match replayed {
            0 => String::new(),
            1 => ", 1 later commit replayed".to_string(),
            n => format!(", {} later commits replayed", n),
        },
        &new_head[..7]
    );
    println!("`rgit undo` takes it back (`rgit recover` lists dropped commits too).");
    if stashed {
        crate::workspace::state_files::stash_apply_or_pop(None, true)?;
        println!("Your uncommitted changes are back (unstaged, as after `rebase --autostash`).");
    }
    Ok(())
}
