use anyhow::Result;

/// `rgit pull [<remote>] [<branch>]`
///
/// Fetches from and integrates with another repository or a local branch.
///
/// In its default form, `rgit pull` is shorthand for `rgit fetch` followed by `rgit merge FETCH_HEAD`
/// (or the remote-tracking branch). More specifically, it:
/// 1. Verifies that the current repository state is on a local branch (not detached HEAD).
/// 2. Resolves the remote repository name (from CLI argument, tracking configuration
///    `branch.<current>.remote`, or defaults to `"origin"`).
/// 3. Resolves the target remote branch to integrate (from CLI argument, tracking configuration
///    `branch.<current>.merge`, or defaults to the current branch name).
/// 4. Executes `fetch` to download all necessary commits/trees/blobs from the remote and update
///    the remote-tracking branch under `refs/remotes/<remote>/<branch>`.
/// 5. Executes `merge` on the remote-tracking ref into the currently checked-out branch,
///    performing fast-forward updates or 3-way merges as appropriate.
// Used by CLI command dispatch (`cli.rs`).
pub fn pull(remote: Option<String>, branch: Option<String>) -> Result<()> {
    let current_branch = match crate::refs::resolve_head()? {
        crate::refs::HeadState::Branch(b) => b,
        crate::refs::HeadState::Detached(_) => {
            anyhow::bail!("fatal: You are not currently on a branch. Please specify which branch you want to merge with.");
        }
    };

    let remote_name = match remote {
        Some(r) => r,
        None => {
            crate::helpers::config::get_config_value(&format!("branch.{}.remote", current_branch))?
                .unwrap_or_else(|| "origin".to_string())
        }
    };

    let merge_branch = match branch {
        Some(b) => b,
        None => {
            let configured_merge =
                crate::helpers::config::get_config_value(&format!("branch.{}.merge", current_branch))?;
            if let Some(m) = configured_merge {
                m.strip_prefix("refs/heads/").unwrap_or(&m).to_string()
            } else {
                current_branch.clone()
            }
        }
    };

    crate::commands::fetch(Some(remote_name.clone()), Some(merge_branch.clone()))?;

    let tracking_ref = format!("{}/{}", remote_name, merge_branch);
    crate::commands::merge(tracking_ref)?;

    Ok(())
}
