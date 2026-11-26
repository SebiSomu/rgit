pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;

use crate::core::refs;
use crate::workspace::reword::engine::{reword_chain, rewrite_chain};

/// `rgit reword [<commit>]`: changes the message of HEAD or an earlier commit on the current
/// branch, without an interactive rebase. The new message is `message` (`-m`) or what the
/// editor saves from the current one. Only messages change, so the commits after it are
/// rewritten without any conflict, and the index and working tree (staged changes included)
/// are left alone. Refuses to rewrite pushed commits unless `force`.
// Used by `rgit reword`.
pub fn reword(commit: Option<String>, message: Option<String>, force: bool) -> Result<()> {
    crate::workspace::fixup::refuse_if_busy();
    let head_state = refs::resolve_head()?;
    let Some(head) = refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits to reword yet");
    };
    let rev = commit.as_deref().unwrap_or("HEAD");
    let target = match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", rev)) {
        Ok(hash) => hash,
        Err(_) => crate::ui::die_fatal(format!("'{}' is not a commit", rev)),
    };
    let chain = match reword_chain(&head, &target) {
        Ok(chain) => chain,
        Err(e) => crate::ui::die_fatal(e.to_string().trim_start_matches("fatal: ")),
    };
    let old = &chain.commits[0];
    let short = &target[..7];
    let old_subject = old.message.lines().next().unwrap_or("").to_string();
    if !force && let Some(remote) = crate::workspace::fixup::engine::pushed_on(&target)? {
        crate::ui::die_fatal(format!(
            "{} \"{}\" is already pushed ({} has it); rewording it would make the branches diverge\nhint: pass --force to reword it anyway (you'll need `rgit push --force` afterwards)",
            short, old_subject, remote
        ));
    }

    let new_message = match message {
        Some(text) => crate::core::commit::cleanup_commit_message(&text, false),
        None => {
            let later = chain.commits.len() - 1;
            let comments = format!(
                "# Rewording {} \"{}\"{}.\n",
                short,
                old_subject,
                match later {
                    0 => String::new(),
                    1 => " (the 1 commit after it is rewritten too, with the same changes)".to_string(),
                    n => format!(" (the {} commits after it are rewritten too, with the same changes)", n),
                }
            );
            let saved = crate::core::commit::edit_commit_message(&old.message, &comments)?;
            crate::core::commit::cleanup_commit_message(&saved, true)
        }
    };
    if new_message.trim().is_empty() {
        crate::ui::die(1, "Aborting reword due to empty commit message; nothing changed.");
    }
    if new_message == old.message {
        println!("The message of {} is unchanged; nothing to do.", short);
        return Ok(());
    }

    let committer = crate::core::commit::current_signature()?;
    let new_subject = crate::core::commit::split_commit_message(&new_message).0;
    crate::core::reflog::set_reflog_action(&format!("reword: {}", new_subject));
    let written = rewrite_chain(&chain, &new_message, &committer)?;
    let new_head = written.last().cloned().unwrap_or_default();
    match &head_state {
        refs::HeadState::Branch(branch) => refs::write_ref(&format!("refs/heads/{}", branch), &new_head)?,
        refs::HeadState::Detached(_) => refs::set_head_detached(&new_head)?,
    }
    let later = written.len() - 1;
    println!(
        "Reworded {} -> {} \"{}\"{}.",
        short,
        &written[0][..7],
        new_subject,
        match later {
            0 => String::new(),
            1 => ", 1 later commit rewritten".to_string(),
            n => format!(", {} later commits rewritten", n),
        }
    );
    println!("(was \"{}\"; `rgit undo` takes it back)", old_subject);
    Ok(())
}
