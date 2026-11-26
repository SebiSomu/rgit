//! The parts of `rgit reword` that find the commits to rewrite and write the new ones.

use anyhow::Result;

use crate::core::commit::{parse_commit_object, write_commit_object};
use crate::core::types::CommitSignature;
use crate::workspace::reword::types::RewordChain;

/// The commits from `target` to `head`, oldest first, following HEAD's first parents. Fails
/// when `target` is not on that line (another branch, or only behind a merge's other side).
// Used by `rgit reword`.
pub fn reword_chain(head: &str, target: &str) -> Result<RewordChain> {
    let mut commits = Vec::new();
    let mut current = Some(head.to_string());
    while let Some(hash) = current {
        let commit = parse_commit_object(&hash)?;
        current = commit.parents.first().cloned();
        let found = hash == target;
        commits.push(commit);
        if found {
            commits.reverse();
            return Ok(RewordChain { commits });
        }
    }
    anyhow::bail!(
        "fatal: {} is not in the current branch's first-parent history (`rgit log --first-parent` lists what can be reworded)",
        &target[..target.len().min(7)]
    )
}

/// Writes the chain again with `message` on its first commit: the same trees, authors and
/// (for later commits) messages, each later commit on the new one before it, the committer
/// being `committer`, as a rebase would. Merge commits keep their other parents. Returns the
/// new commits, oldest first (the last one is the new HEAD).
// Used by `rgit reword`.
pub fn rewrite_chain(chain: &RewordChain, message: &str, committer: &CommitSignature) -> Result<Vec<String>> {
    let mut written: Vec<String> = Vec::with_capacity(chain.commits.len());
    for (i, commit) in chain.commits.iter().enumerate() {
        let mut parents = commit.parents.clone();
        if let Some(previous) = written.last() {
            parents[0] = previous.clone();
        }
        let text = if i == 0 { message } else { commit.message.as_str() };
        written.push(write_commit_object(&commit.tree, &parents, &commit.author, committer, text)?);
    }
    Ok(written)
}
