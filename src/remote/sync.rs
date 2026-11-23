pub mod cli;
pub mod engine;
pub mod types;

use crate::core::refs;
use crate::remote::sync::engine::{commits, integration_mode, measure, print_plan, upstream_of};
use crate::remote::sync::types::Integration;

/// `rgit sync [--push] [--rebase | --merge] [--dry-run] [--no-fetch]`: fetch the branch's
/// upstream, print where the branch stands and the plan, then fast-forward, rebase (default) or
/// merge onto it and, with `--push`, push the result (a plain fast-forward push: after the
/// rebase or merge the branch contains the upstream). Unstaged and staged changes are stashed
/// around the rebase or merge. A branch with no upstream is published with `--push`.
pub fn sync(flag: Option<Integration>, push: bool, dry_run: bool, fetch: bool) -> anyhow::Result<()> {
    let branch = match refs::resolve_head()? {
        refs::HeadState::Branch(b) => b,
        refs::HeadState::Detached(_) => crate::ui::die_fatal("HEAD is detached; switch to the branch you want to sync"),
    };
    crate::workspace::fixup::refuse_if_busy();
    let mut state = upstream_of(&branch)?;
    if crate::core::config::get_config_value(&format!("remote.{}.url", state.remote))?.is_none() {
        crate::ui::die_fatal(format!("'{}' has no upstream and there is no remote '{}'\nhint: add one with `rgit remote add {} <url>`", branch, state.remote, state.remote));
    }
    if fetch && state.configured {
        crate::remote::fetch::fetch(Some(state.remote.clone()), Some(state.remote_branch.clone()))?;
    }
    measure(&mut state)?;
    let mode = integration_mode(&branch, flag)?;
    let work = print_plan(&state, mode, push);
    if state.tracking_tip.is_none() && !push {
        let hint = if state.configured {
            format!("hint: its upstream {} is gone; `rgit sync --push` publishes it again, `rgit branch --unset-upstream` forgets it", state.tracking_name())
        } else {
            format!("hint: `rgit sync --push` publishes it to {} and tracks it", state.remote)
        };
        println!("{}", hint);
        return Ok(());
    }
    if dry_run {
        println!("(dry run: nothing changed{})", if fetch && state.configured { " except the fetch" } else { "" });
        return Ok(());
    }
    if !work {
        if state.ahead > 0 {
            println!("Not pushed yet: {} (`rgit sync --push`).", commits(state.ahead));
        }
        return Ok(());
    }

    // Publishing a branch that has no upstream yet.
    if state.tracking_tip.is_none() {
        crate::remote::push::push(Some(state.remote.clone()), Some(format!("{}:{}", branch, state.remote_branch)), false, true)?;
        return Ok(());
    }

    if state.behind > 0 {
        integrate(&state, mode)?;
        measure(&mut state)?;
    }
    if push && state.ahead > 0 {
        crate::remote::push::push(Some(state.remote.clone()), Some(format!("{}:{}", branch, state.remote_branch)), false, false)?;
        measure(&mut state)?;
    }
    if state.ahead > 0 {
        println!("'{}' now contains {}; not pushed yet: {} (`rgit sync --push`).", branch, state.tracking_name(), commits(state.ahead));
    } else {
        println!("'{}' is in sync with {}.", branch, state.tracking_name());
    }
    Ok(())
}

/// Brings the upstream's commits in: a fast-forward when the branch has none of its own, else
/// a rebase or a merge. Local changes are stashed around it; a conflict stops with a note on
/// how to go on or back (`rgit undo`).
// Used by `sync`.
fn integrate(state: &crate::remote::sync::types::SyncState, mode: Integration) -> anyhow::Result<()> {
    use crate::merge::merge::types::{FastForwardMode, MergeOptions};
    let head = refs::resolve_head_commit()?.unwrap_or_default();
    let dirty = !crate::core::commit::tracked_worktree_changes()?.is_empty() || crate::workspace::fixup::engine::has_staged_changes(&head)?;
    if dirty {
        crate::workspace::stash::stash_push(Some("rgit sync: local changes".to_string()), None)?;
    }
    let up = state.tracking_name();
    let how = if state.ahead == 0 { "fast-forward" } else if mode == Integration::Rebase { "rebase" } else { "merge" };
    let go_on = match how {
        "rebase" => "resolve the conflicts, `rgit add` them and `rgit rebase --continue`",
        _ => "resolve the conflicts, `rgit add` them and `rgit commit`",
    };
    crate::ui::note_on_failure(Some(format!(
        "rgit sync stopped during the {} onto {}: {}{};\nor `rgit undo` to go back to before the sync.",
        how,
        up,
        go_on,
        if dirty { ", then `rgit stash pop` (your local changes)" } else { "" }
    )));
    match how {
        "fast-forward" => crate::merge::merge::merge(MergeOptions { commits: vec![up.clone()], ff: FastForwardMode::Only, ..Default::default() })?,
        "rebase" => crate::merge::rebase::rebase(crate::merge::rebase::types::RebaseOptions { upstream: Some(up.clone()), ..Default::default() })?,
        _ => crate::merge::merge::merge(MergeOptions { commits: vec![up.clone()], ..Default::default() })?,
    }
    if crate::core::repo::path(".git/MERGE_HEAD").exists() {
        crate::ui::exit(1);
    }
    crate::ui::note_on_failure(None);
    if dirty {
        crate::workspace::state_files::stash_apply_or_pop(None, true)?;
    }
    Ok(())
}
