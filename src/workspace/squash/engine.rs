//! The parts of `rgit squash` that decide what to fold and what message to propose.

use anyhow::Result;

use crate::core::commit::parse_commit_object;
use crate::workspace::squash::types::SquashPlan;

/// The last `count` commits on HEAD's first-parent line, oldest first, with the commit they
/// sit on. Fails for fewer than two commits, more than the branch has, or a merge commit in
/// the range (its other side can't be folded into one parent).
// Used by `rgit squash`.
pub fn plan_squash(head: &str, count: usize) -> Result<SquashPlan> {
    if count < 2 {
        anyhow::bail!("fatal: squashing needs at least 2 commits (`rgit squash 2` folds HEAD into the commit before it)");
    }
    let mut commits = Vec::with_capacity(count);
    let mut current = Some(head.to_string());
    while commits.len() < count {
        let Some(hash) = current else {
            anyhow::bail!("fatal: there are only {} commits on this branch (`rgit squash {}` folds them all)", commits.len(), commits.len());
        };
        let commit = parse_commit_object(&hash)?;
        if commit.parents.len() > 1 {
            anyhow::bail!(
                "fatal: {} \"{}\" is a merge commit; rgit can't fold a merge (squash the commits after it)",
                &hash[..7],
                commit.message.lines().next().unwrap_or("")
            );
        }
        current = commit.parents.first().cloned();
        commits.push(commit);
    }
    commits.reverse();
    Ok(SquashPlan { commits, base: current })
}

/// The message to propose for the squashed commit. For the editor (`for_editor`), git's
/// interactive-rebase template: a comment header per commit, `fixup!` commits' messages
/// (after the first) commented out. Otherwise the messages themselves, oldest first, blank-line separated,
/// without the `fixup!` ones.
// Used by `rgit squash`.
pub fn combined_message(plan: &SquashPlan, for_editor: bool) -> String {
    let skipped = |message: &str| message.starts_with("fixup! ");
    let messages: Vec<&str> = plan.commits.iter().map(|c| c.message.trim_end()).collect();
    if !for_editor {
        // The first commit's message always stays, as in the editor template.
        let kept: Vec<&str> = messages.iter().enumerate().filter(|(i, m)| *i == 0 || !skipped(m)).map(|(_, m)| *m).collect();
        return format!("{}\n", kept.join("\n\n"));
    }
    let mut out = format!("# This is a combination of {} commits.\n", messages.len());
    for (i, message) in messages.iter().enumerate() {
        let n = i + 1;
        if skipped(message) && i > 0 {
            out.push_str(&format!("# The commit message #{} will be skipped:\n\n", n));
            for line in message.lines() {
                out.push_str(&format!("# {}\n", line).replace("# \n", "#\n"));
            }
        } else {
            let label = if n == 1 { "# This is the 1st commit message:".to_string() } else { format!("# This is the commit message #{}:", n) };
            out.push_str(&format!("{}\n\n{}\n", label, message));
        }
        if n < messages.len() {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
#[path = "../../../tests/unit/workspace/squash.rs"]
mod tests;
