//! Helpers of `rgit sync`: the branch's upstream, ahead/behind counts, rebase-or-merge choice
//! and the printed plan.

use anyhow::Result;

use crate::core::config::get_config_value;
use crate::core::refs;
use crate::remote::sync::types::{Integration, SyncState};

/// The current branch's upstream (`branch.<b>.remote` / `.merge`), or, when none is set, the
/// remote it would be published to (`branch.<b>.remote`, else `origin`) under its own name.
// Used by the `sync` command.
pub fn upstream_of(branch: &str) -> Result<SyncState> {
    let remote = get_config_value(&format!("branch.{}.remote", branch))?;
    let merge = get_config_value(&format!("branch.{}.merge", branch))?;
    let configured = remote.is_some() && merge.is_some();
    let remote = remote.unwrap_or_else(|| "origin".to_string());
    let remote_branch = merge.map(|m| m.strip_prefix("refs/heads/").unwrap_or(&m).to_string()).unwrap_or_else(|| branch.to_string());
    Ok(SyncState { branch: branch.to_string(), remote, remote_branch, configured, tracking_tip: None, ahead: 0, behind: 0 })
}

/// Fills in the remote-tracking tip and the ahead/behind counts against it.
// Used by the `sync` command (before integrating, and again before pushing).
pub fn measure(state: &mut SyncState) -> Result<()> {
    let head = refs::read_ref(&format!("refs/heads/{}", state.branch))?.unwrap_or_default();
    state.tracking_tip = refs::read_ref(&format!("refs/remotes/{}", state.tracking_name()))?;
    let (ahead, behind) = match &state.tracking_tip {
        Some(tip) => {
            let ours = crate::core::commit::collect_ancestors(&head)?;
            let theirs = crate::core::commit::collect_ancestors(tip)?;
            (ours.difference(&theirs).count(), theirs.difference(&ours).count())
        }
        None => (0, 0),
    };
    state.ahead = ahead;
    state.behind = behind;
    Ok(())
}

/// `--rebase` / `--merge` when given; else `branch.<b>.rebase`, then `pull.rebase` (git's
/// `true`/`merges`/`interactive` mean rebase, `false` means merge); else rebase.
// Used by the `sync` command.
pub fn integration_mode(branch: &str, flag: Option<Integration>) -> Result<Integration> {
    if let Some(mode) = flag {
        return Ok(mode);
    }
    for key in [format!("branch.{}.rebase", branch), "pull.rebase".to_string()] {
        if let Some(value) = get_config_value(&key)? {
            return Ok(if value.eq_ignore_ascii_case("false") { Integration::Merge } else { Integration::Rebase });
        }
    }
    Ok(Integration::Rebase)
}

/// `1 commit` / `3 commits`.
// Used by `print_plan` and the `sync` command.
pub fn commits(n: usize) -> String {
    format!("{} commit{}", n, if n == 1 { "" } else { "s" })
}

/// Prints where the branch stands and what `sync` will do about it. Returns whether there is
/// anything to do.
// Used by the `sync` command.
pub fn print_plan(state: &SyncState, mode: Integration, push: bool) -> bool {
    let up = state.tracking_name();
    let Some(_) = &state.tracking_tip else {
        // Not published (or the remote branch is gone): only --push can do something.
        println!("'{}' has no upstream on {} yet.", state.branch, state.remote);
        if push {
            println!("Plan:\n  - push '{}' to {} and track {}", state.branch, state.remote, up);
        }
        return push;
    };
    let standing = match (state.ahead, state.behind) {
        (0, 0) => format!("'{}' is up to date with {}.", state.branch, up),
        (a, 0) => format!("'{}' is {} ahead of {}.", state.branch, commits(a), up),
        (0, b) => format!("'{}' is {} behind {}.", state.branch, commits(b), up),
        (a, b) => format!("'{}' and {} have diverged: {} here, {} there.", state.branch, up, commits(a), commits(b)),
    };
    println!("{}", standing);
    let mut steps = Vec::new();
    match (state.ahead, state.behind) {
        (_, 0) => {}
        (0, _) => steps.push(format!("fast-forward to {}", up)),
        (a, _) => steps.push(match mode {
            Integration::Rebase => format!("rebase {} onto {}", if a == 1 { "the local commit".to_string() } else { format!("the {} local commits", a) }, up),
            Integration::Merge => format!("merge {} into '{}'", up, state.branch),
        }),
    }
    if push && (state.ahead > 0) {
        steps.push(format!("push to {}", up));
    }
    if !steps.is_empty() {
        println!("Plan:");
        for step in &steps {
            println!("  - {}", step);
        }
    }
    !steps.is_empty()
}
