pub mod cli;
pub mod engine;
pub mod types;

use std::collections::BTreeMap;
use crate::workspace::checkout::{check_switch_safety, ensure_working_tree_clean, sync_working_tree, update_index_from_tree};
use crate::core::commit::resolve_commit_from_source;
use crate::merge::merge::engine::find_all_merge_bases;
use crate::core::commit::resolve_tree_from_source;
use crate::merge::rebase::engine::{
    begin_replay, can_fast_forward, cleanup_rebase_state, edit_todo, rebase_abort, rebase_continue,
    rebase_edit_todo, rebase_in_progress, rebase_skip, select_commits_to_rebase, subject_of,
};
use crate::core::index::read_index;
use crate::merge::rebase::types::{Action, RebaseOptions, TodoItem};
use crate::core::refs;

fn die(code: i32, message: &str) -> ! {
    eprintln!("{}", message);
    crate::ui::exit(code);
}

/// `rgit rebase [-i] [--onto <newbase> | --keep-base] [<upstream> [<branch>]]` /
/// `--continue` / `--abort` / `--skip` / `--edit-todo`
///
/// Replays the branch's commits that `<upstream>` lacks (git's `<upstream>...HEAD` right side:
/// merges left out, commits whose patch upstream already has skipped with git's warning unless
/// `--reapply-cherry-picks`) onto `--onto <newbase>`, the merge base with `--keep-base`, or
/// `<upstream>` itself. `<branch>` is switched to first; without `<upstream>` the branch's
/// configured upstream is used. Nothing is rewritten when the branch already sits on top of the
/// new base ("Current branch <b> is up to date."). Progress, conflicts, the final message and
/// the rewritten commits (original author, committer now) follow git 2.43.
pub fn rebase(opts: RebaseOptions) -> anyhow::Result<()> {
    let mode_count = [opts.abort, opts.cont, opts.skip, opts.edit_todo].iter().filter(|&&b| b).count();
    if mode_count > 1 {
        anyhow::bail!("fatal: cannot combine --continue, --abort, --skip, or --edit-todo");
    }
    if opts.abort {
        return rebase_abort();
    }
    if opts.cont {
        return rebase_continue();
    }
    if opts.skip {
        return rebase_skip();
    }
    if opts.edit_todo {
        return rebase_edit_todo();
    }

    if rebase_in_progress() {
        anyhow::bail!(
            "fatal: a rebase is already in progress\n\
             hint: use 'rgit rebase --continue' or 'rgit rebase --abort'"
        );
    }

    // git resolves <upstream>, then --onto, then <branch>.
    let upstream_name = match &opts.upstream {
        Some(name) => name.clone(),
        None => match crate::history::cherry::engine::resolve_cherry_upstream()? {
            Some(tracking) => tracking,
            None => {
                print_no_upstream_help()?;
                crate::ui::exit(1);
            }
        },
    };
    let upstream = match resolve_commit_from_source(&upstream_name) {
        Ok(commit) => commit,
        Err(_) => die(128, &format!("fatal: invalid upstream '{}'", upstream_name)),
    };
    let onto_arg = match &opts.onto {
        Some(name) => match resolve_commit_from_source(name) {
            Ok(commit) => Some(commit),
            Err(_) => die(128, &format!("fatal: Does not point to a valid commit '{}'", name)),
        },
        None => None,
    };
    // <branch>: a branch name keeps the rebase on that branch; any other commit detaches.
    let (head_state, orig_head) = match &opts.branch {
        Some(name) => {
            if let Ok(Some(tip)) = refs::read_ref(&format!("refs/heads/{}", name)) {
                (refs::HeadState::Branch(name.clone()), tip)
            } else {
                match resolve_commit_from_source(name) {
                    Ok(commit) => (refs::HeadState::Detached(commit.clone()), commit),
                    Err(_) => die(128, &format!("fatal: no such branch/commit '{}'", name)),
                }
            }
        }
        None => {
            let state = refs::resolve_head()?;
            let head = refs::resolve_head_commit()?.ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
            (state, head)
        }
    };
    let onto = match (onto_arg, opts.keep_base) {
        (Some(onto), _) => onto,
        (None, true) => {
            let bases = find_all_merge_bases(&upstream, &orig_head)?;
            if bases.len() != 1 {
                die(128, &format!("fatal: '{}': need exactly one merge base with branch", upstream_name));
            }
            bases[0].clone()
        }
        (None, false) => upstream.clone(),
    };

    if crate::merge::merge::engine::report_needs_merge() {
        eprintln!("error: cannot rebase: You have unstaged changes.");
        eprintln!("error: additionally, your index contains uncommitted changes.");
        eprintln!("error: Please commit or stash them.");
        crate::ui::exit(1);
    }
    ensure_working_tree_clean("start a rebase")?;

    if !opts.interactive && can_fast_forward(&onto, &upstream, &orig_head)? {
        switch_to(&head_state, &orig_head)?;
        match &head_state {
            refs::HeadState::Branch(branch) => println!("Current branch {} is up to date.", branch),
            refs::HeadState::Detached(_) => println!("HEAD is up to date."),
        }
        return Ok(());
    }

    // --keep-base implies --reapply-cherry-picks.
    let reapply = opts.reapply_cherry_picks.unwrap_or(opts.keep_base);
    let (commits, skipped) = select_commits_to_rebase(&upstream, &orig_head, reapply)?;
    if !skipped.is_empty() && !opts.interactive {
        for commit in &skipped {
            eprintln!("warning: skipped previously applied commit {}", crate::history::rev_list::engine::abbreviate_hash(commit));
        }
        eprintln!("hint: use --reapply-cherry-picks to include skipped commits");
        eprintln!("hint: Disable this message with \"rgit config advice.skippedCherryPicks false\"");
    }
    let mut items: Vec<TodoItem> = Vec::new();
    for hash in commits {
        let subject = subject_of(&hash)?;
        items.push(TodoItem { action: Action::Pick, hash, subject });
    }

    if opts.interactive {
        match edit_todo(&items, &upstream, &orig_head, &onto) {
            Ok(edited) => items = edited,
            Err(e) => {
                cleanup_rebase_state();
                return Err(e);
            }
        }
        if items.is_empty() {
            cleanup_rebase_state();
            println!("Nothing to do.");
            return Ok(());
        }
    }

    begin_replay(&head_state, &orig_head, &onto, items, opts.interactive)
}

/// Checks out `<branch>` when it is not what `HEAD` is now (git's `checkout_up_to_date`).
fn switch_to(head_state: &refs::HeadState, tip: &str) -> anyhow::Result<()> {
    let current_state = refs::resolve_head()?;
    let current = refs::resolve_head_commit()?;
    let same = match (head_state, &current_state) {
        (refs::HeadState::Branch(a), refs::HeadState::Branch(b)) => a == b,
        (refs::HeadState::Detached(_), refs::HeadState::Detached(_)) => current.as_deref() == Some(tip),
        _ => false,
    };
    if same {
        return Ok(());
    }
    let target_tree = resolve_tree_from_source(tip)?;
    let head_tree = match &current {
        Some(commit) => resolve_tree_from_source(commit)?,
        None => BTreeMap::new(),
    };
    let index_map: BTreeMap<String, [u8; 20]> = read_index().unwrap_or_default().iter().map(|e| (e.path.clone(), e.hash)).collect();
    check_switch_safety(&target_tree, &head_tree, &index_map)?;
    sync_working_tree(&target_tree, &head_tree, &index_map)?;
    update_index_from_tree(&target_tree, &head_tree)?;
    match head_state {
        refs::HeadState::Branch(branch) => refs::set_head(branch),
        refs::HeadState::Detached(_) => refs::set_head_detached(tip),
    }
}

/// git's message when no `<upstream>` is given and the branch has none configured (stdout).
fn print_no_upstream_help() -> anyhow::Result<()> {
    match refs::resolve_head()? {
        refs::HeadState::Branch(branch) => {
            println!("There is no tracking information for the current branch.");
            println!("Please specify which branch you want to rebase against.");
            println!("See git-rebase(1) for details.");
            println!();
            println!("    rgit rebase '<branch>'");
            println!();
            println!("If you wish to set tracking information for this branch you can do so with:");
            println!();
            println!("    rgit branch --set-upstream-to=<remote>/<branch> {}", branch);
            println!();
        }
        refs::HeadState::Detached(_) => {
            println!("You are not currently on a branch.");
            println!("Please specify which branch you want to rebase against.");
            println!("See git-rebase(1) for details.");
            println!();
            println!("    rgit rebase '<branch>'");
            println!();
        }
    }
    Ok(())
}
