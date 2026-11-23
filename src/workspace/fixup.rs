pub mod cli;
pub mod engine;
pub mod types;

use crate::core::refs;
use crate::workspace::fixup::engine::{PICKER_LIMIT, candidates, fixup_todo, has_staged_changes, pick_interactively, pushed_on};

/// `rgit fixup [<commit>] [-a] [-f]`: folds the staged changes into an earlier commit in one
/// step (git: `commit --fixup` + `rebase -i --autosquash` + stash around it). Without
/// `<commit>`, a numbered list of recent commits to pick from. HEAD is simply amended; an older
/// commit gets a `fixup!` commit that is squashed into it right away, rewriting the commits on
/// top. Unstaged changes are stashed for the rewrite and popped afterwards. Commits a
/// remote-tracking branch already has are refused unless `--force`.
pub fn fixup(commit: Option<String>, all: bool, force: bool) -> anyhow::Result<()> {
    refuse_if_busy();
    let head_state = refs::resolve_head()?;
    let Some(head) = refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits to fix up yet");
    };
    if all {
        crate::core::commit::stage_tracked_changes()?;
    }
    crate::merge::merge::engine::die_if_unmerged_for_commit()?;
    if !has_staged_changes(&head)? {
        crate::ui::die_fatal("nothing staged to fix up with\nhint: stage the changes with `rgit add <path>`, or use `rgit fixup -a`");
    }

    let target = match commit {
        Some(rev) => match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", rev)) {
            Ok(hash) => hash,
            Err(_) => crate::ui::die_fatal(format!("'{}' is not a commit", rev)),
        },
        None => {
            let list = candidates(&head, PICKER_LIMIT)?;
            match pick_interactively(&list)? {
                Some(hash) => hash,
                None => {
                    println!("Cancelled; nothing changed.");
                    return Ok(());
                }
            }
        }
    };
    let target_commit = crate::core::commit::parse_commit_object(&target)?;
    let subject = target_commit.message.lines().next().unwrap_or("").to_string();
    let short = target[..7].to_string();
    if !crate::core::commit::is_reachable(&head, &target)? {
        crate::ui::die_fatal(format!("{} is not in the current branch's history", short));
    }
    if !force && let Some(remote) = pushed_on(&target)? {
        crate::ui::die_fatal(format!(
            "{} is already pushed ({} has it); rewriting it would make the branches diverge\nhint: pass --force to rewrite it anyway (you'll need `rgit push --force` afterwards)",
            short, remote
        ));
    }

    // HEAD itself: a plain amend, no rewrite of anything else.
    if target == head {
        crate::workspace::commit::commit(quiet_commit(Vec::new(), true))?;
        let new_head = refs::resolve_head_commit()?.unwrap_or_default();
        println!("Fixed up {} {} (amended HEAD, now {}).", short, subject, &new_head[..7]);
        return Ok(());
    }
    let Some(target_parent) = target_commit.parents.first().cloned() else {
        crate::ui::die_fatal("the root commit can only be fixed up while it is HEAD");
    };
    if target_commit.parents.len() > 1 {
        crate::ui::die_fatal(format!("{} is a merge commit; rgit can't replay merges", short));
    }

    crate::workspace::commit::commit(quiet_commit(vec![format!("fixup! {}", subject)], false))?;
    let fixup_commit = refs::resolve_head_commit()?.unwrap_or_default();
    let items = match fixup_todo(&fixup_commit, &target, &target_parent) {
        Ok(items) => items,
        Err(e) => {
            eprintln!("{}", e);
            crate::ui::die_fatal(format!("can't fold the fixup into {}: a merge sits between it and HEAD (the fixup! commit {} is kept)", short, &fixup_commit[..7]));
        }
    };
    let rewritten = items.len() - 2;

    // Unstaged changes would block the rewrite: park them in the stash meanwhile.
    let stashed = !crate::core::commit::tracked_worktree_changes()?.is_empty();
    if stashed {
        crate::workspace::stash::stash_push(Some("rgit fixup: unstaged changes".to_string()), None)?;
    }

    crate::ui::note_on_failure(Some(format!(
        "rgit fixup stopped while folding the fix into {}: resolve the conflicts, `rgit add` them and `rgit rebase --continue`{};\nor `rgit undo` to go back to before the fixup (your staged fix staged again).",
        short,
        if stashed { ", then `rgit stash pop` (your unstaged changes)" } else { "" }
    )));
    crate::merge::rebase::engine::begin_replay(&head_state, &fixup_commit, &target_parent, items, false)?;
    crate::ui::note_on_failure(None);

    let mut new_target = refs::resolve_head_commit()?.unwrap_or_default();
    for _ in 0..rewritten {
        new_target = crate::core::commit::parse_commit_object(&new_target)?.parents[0].clone();
    }
    println!(
        "Fixed up {} {} (now {}){}.",
        short,
        subject,
        &new_target[..7],
        match rewritten {
            1 => ", 1 later commit rewritten".to_string(),
            n => format!(", {} later commits rewritten", n),
        }
    );
    if stashed {
        crate::workspace::state_files::stash_apply_or_pop(None, true)?;
    }
    Ok(())
}

/// A commit that never opens the editor and prints nothing: `-m <message>` or `--amend --no-edit`.
// Used by `fixup`.
fn quiet_commit(messages: Vec<String>, amend: bool) -> crate::workspace::commit::types::CommitOptions {
    crate::workspace::commit::types::CommitOptions { messages, amend, no_edit: amend, quiet: true, ..Default::default() }
}

/// Refuses while a merge, rebase, cherry-pick, revert or am is in progress.
// Used by `fixup` and `sync`.
pub(crate) fn refuse_if_busy() {
    let state = |p: &str| crate::core::repo::path(p).exists();
    let busy = if crate::merge::rebase::engine::rebase_in_progress() {
        Some("rebase")
    } else if state(".git/MERGE_HEAD") {
        Some("merge")
    } else if state(".git/CHERRY_PICK_HEAD") {
        Some("cherry-pick")
    } else if state(".git/REVERT_HEAD") {
        Some("revert")
    } else if state(".git/rebase-apply") {
        Some("am")
    } else {
        None
    };
    if let Some(op) = busy {
        crate::ui::die_fatal(format!("a {} is in progress; finish or abort it first", op));
    }
}
