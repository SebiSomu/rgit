//! Narration printed before a command acts: warnings the user should see first (e.g. pushing a protected branch).

use crate::ui::{
    {danger, detail, has, plural, positionals},
    narrate::{commits_between, is_ancestor},
    types::RepoSnapshot,
};

pub(super) fn before_command(command: &str, args: &[String], b: &RepoSnapshot) {
    match command {
        "push" => {
            let Some(branch) = &b.head_branch else { return };
            let pos = positionals(args, &[]);
            let remote = pos.first().cloned().unwrap_or_else(|| crate::core::config::get_config_value(&format!("branch.{}.remote", branch)).ok().flatten().unwrap_or_else(|| "origin".into()));
            let tracking = format!("refs/remotes/{}/{}", remote, branch);
            if has(args, &["-f", "--force"]) {
                if let (Some(theirs), Some(ours)) = (b.refs.get(&tracking), &b.head) {
                    if !is_ancestor(theirs, ours) {
                        let lost = commits_between(Some(ours), theirs);
                        danger(&format!("force-pushing over {} on {}/{} that your branch doesn't have; they'll be gone from the remote.", plural(lost, "commit", "commits"), remote, branch));
                        detail("If someone else pushed them, they lose their work. Safer: `rgit pull` first, then push normally.");
                    }
                }
            }
            if ["main", "master"].contains(&branch.as_str()) {
                detail(&format!("You're pushing straight to '{}', the default branch.", branch));
            }
        }
        "diff" if positionals(args, &[]).is_empty() => {
            if has(args, &["--cached", "--staged"]) {
                detail("Comparing the index with HEAD: these are your STAGED changes (what `rgit commit` would record).");
            } else {
                detail("Comparing the working tree with the index: these are your UNSTAGED changes (`--cached` shows the staged ones).");
            }
        }
        "reset" if has(args, &["--hard"]) => {
            let changed = b.worktree_changes.as_ref().map(|c| c.len()).unwrap_or(0);
            if changed + b.staged.len() > 0 {
                detail("--hard overwrites the index AND your files: uncommitted work will be lost.");
            }
        }
        _ => {}
    }
}

// ----------------------------------------------------------------------------------------
// After the command: what actually happened
// ----------------------------------------------------------------------------------------
