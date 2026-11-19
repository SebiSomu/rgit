use crate::core::{refs, commit::format_commit_date};
use crate::core::types::CommitSignature;

/// Prints git's commit summary: `[<branch> <abbrev>] <subject>`, then ` Author:` when the
/// author is not the committer, ` Date:` when `show_date` (an author taken from another commit,
/// or `--date`), and for non-merges the diffstat line and create/delete-mode summary.
// Used by `commit` and `cherry-pick`.
pub fn print_commit_summary(head_state: &refs::HeadState, commit_hash: &str, author: &CommitSignature, committer: &CommitSignature, show_date: bool) -> anyhow::Result<()> {
    use crate::core::commit::{parse_commit_object, split_commit_message};
    let commit = parse_commit_object(commit_hash)?;
    let label = match head_state {
        refs::HeadState::Branch(branch) => branch.clone(),
        refs::HeadState::Detached(_) => "detached HEAD".to_string(),
    };
    let root = if commit.parents.is_empty() { " (root-commit)" } else { "" };
    let subject = split_commit_message(&commit.message).0;
    println!("[{}{} {}] {}", label, root, &commit_hash[..7], subject);
    if author.name != committer.name || author.email != committer.email {
        println!(" Author: {} <{}>", author.name, author.email);
    }
    if show_date {
        println!(" Date: {}", format_commit_date(author.timestamp, &author.tz));
    }
    if commit.parents.len() <= 1 {
        // git's summary: `--shortstat --summary` against the parent, renames detected.
        use crate::diff::plumbing::{diff_trees, render_diff_changes};
        let opts = crate::diff::plumbing::types::DiffPlumbingOptions {
            recursive: true,
            shortstat: true,
            summary: true,
            renames: crate::diff::rename::default_rename_options("diff.renames"),
            ..Default::default()
        };
        let parent_tree = match commit.parents.first() {
            Some(parent) => Some(parse_commit_object(parent)?.tree),
            None => None,
        };
        let mut changes = Vec::new();
        diff_trees(parent_tree.as_deref(), Some(&commit.tree), "", &opts, &[], &mut changes)?;
        let changes = crate::diff::rename::detect_renames(changes, &opts.renames, &[]);
        print!("{}", render_diff_changes(&changes, &opts)?);
    }
    Ok(())
}
