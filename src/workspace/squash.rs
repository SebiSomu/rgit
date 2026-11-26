pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;

use crate::core::refs;
use crate::workspace::squash::engine::{combined_message, plan_squash};

/// `rgit squash <n>`: folds the last `n` commits into one. The squashed commit has the
/// oldest one's author and parent and HEAD's tree; its message is `message` (`-m`), the
/// messages combined (`--no-edit`), or what the editor saves from the combined template. The
/// index and working tree are left alone. Refuses to rewrite pushed commits unless `force`.
// Used by `rgit squash`.
pub fn squash(count: usize, message: Option<String>, no_edit: bool, force: bool) -> Result<()> {
    crate::workspace::fixup::refuse_if_busy();
    let head_state = refs::resolve_head()?;
    let Some(head) = refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits to squash yet");
    };
    let plan = match plan_squash(&head, count) {
        Ok(plan) => plan,
        Err(e) => crate::ui::die_fatal(e.to_string().trim_start_matches("fatal: ")),
    };
    let oldest = &plan.commits[0];
    if !force && let Some(remote) = crate::workspace::fixup::engine::pushed_on(&oldest.hash)? {
        crate::ui::die_fatal(format!(
            "{} \"{}\" is already pushed ({} has it); squashing it would make the branches diverge\nhint: pass --force to squash anyway (you'll need `rgit push --force` afterwards)",
            &oldest.hash[..7],
            oldest.message.lines().next().unwrap_or(""),
            remote
        ));
    }

    let message = match message {
        Some(text) => crate::core::commit::cleanup_commit_message(&text, false),
        None if no_edit => combined_message(&plan, false),
        None => {
            let comments = format!(
                "# Squashing {} commits ({}..{}) into one, on top of {}.\n",
                plan.commits.len(),
                &oldest.hash[..7],
                &head[..7],
                plan.base.as_deref().map(|b| &b[..7]).unwrap_or("nothing (a new root commit)")
            );
            let saved = crate::core::commit::edit_commit_message(&combined_message(&plan, true), &comments)?;
            crate::core::commit::cleanup_commit_message(&saved, true)
        }
    };
    if message.trim().is_empty() {
        crate::ui::die(1, "Aborting squash due to empty commit message; nothing changed.");
    }

    let tree = plan.commits.last().map(|c| c.tree.clone()).unwrap_or_default();
    let committer = crate::core::commit::current_signature()?;
    let subject = crate::core::commit::split_commit_message(&message).0;
    crate::core::reflog::set_reflog_action(&format!("squash: {} commits into {}", plan.commits.len(), subject));
    let parents: Vec<String> = plan.base.iter().cloned().collect();
    let squashed = crate::core::commit::write_commit_object(&tree, &parents, &oldest.author, &committer, &message)?;
    match &head_state {
        refs::HeadState::Branch(branch) => refs::write_ref(&format!("refs/heads/{}", branch), &squashed)?,
        refs::HeadState::Detached(_) => refs::set_head_detached(&squashed)?,
    }
    crate::workspace::commit_summary::print_commit_summary(&head_state, &squashed, &oldest.author, &committer, true)?;
    println!(
        "Squashed {} commits ({}..{}) into {}. `rgit undo` takes it back.",
        plan.commits.len(),
        &oldest.hash[..7],
        &head[..7],
        &squashed[..7]
    );
    Ok(())
}
