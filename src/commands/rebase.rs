use std::collections::BTreeMap;
use crate::helpers::checkout::{check_switch_safety, ensure_working_tree_clean, sync_working_tree, update_index_from_tree};
use crate::helpers::commit::{resolve_commit_from_source, short_hash};
use crate::helpers::merge::find_merge_base;
use crate::helpers::resolve_tree_from_source;
use crate::helpers::rebase::{
    begin_replay, cleanup_rebase_state, collect_commits_to_replay, edit_todo, rebase_abort,
    rebase_continue, rebase_edit_todo, rebase_in_progress, rebase_skip, subject_of,
};
use crate::index::read_index;
use crate::objects::{Action, TodoItem};
use crate::refs;

/// `rgit rebase [-i] <upstream>` / `--continue` / `--abort` / `--skip` / `--edit-todo`
pub fn rebase(
    upstream: Option<String>,
    interactive: bool,
    cont: bool,
    abort: bool,
    skip: bool,
    edit_todo_flag: bool,
) -> anyhow::Result<()> {
    let mode_count = [abort, cont, skip, edit_todo_flag].iter().filter(|&&b| b).count();
    if mode_count > 1 {
        anyhow::bail!("fatal: cannot combine --continue, --abort, --skip, or --edit-todo");
    }

    if abort {
        return rebase_abort();
    }
    if cont {
        return rebase_continue();
    }
    if skip {
        return rebase_skip();
    }
    if edit_todo_flag {
        return rebase_edit_todo();
    }

    let upstream = upstream.ok_or_else(|| {
        anyhow::anyhow!("fatal: rebase requires an <upstream>, or --continue / --abort / --skip / --edit-todo")
    })?;

    if rebase_in_progress() {
        anyhow::bail!(
            "fatal: a rebase is already in progress\n\
             hint: use 'rgit rebase --continue' or 'rgit rebase --abort'"
        );
    }

    ensure_working_tree_clean("start a rebase")?;

    let head_state = refs::resolve_head()?;
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
    let upstream_commit = resolve_commit_from_source(&upstream)?;

    if our_commit == upstream_commit {
        println!("Current branch is up to date.");
        return Ok(());
    }

    let merge_base = find_merge_base(&our_commit, &upstream_commit)?
        .ok_or_else(|| anyhow::anyhow!("fatal: refusing to rebase unrelated histories"))?;

    if merge_base == upstream_commit && !interactive {
        println!("Current branch is up to date.");
        return Ok(());
    }

    if merge_base == our_commit {
        let target_tree = resolve_tree_from_source(&upstream_commit)?;
        let head_tree = resolve_tree_from_source(&our_commit)?;
        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> =
            index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

        check_switch_safety(&target_tree, &head_tree, &index_map)?;
        sync_working_tree(&target_tree, &head_tree, &index_map)?;
        update_index_from_tree(&target_tree, &head_tree)?;

        match &head_state {
            refs::HeadState::Branch(branch_name) => {
                let ref_path = format!("refs/heads/{}", branch_name);
                refs::write_ref(&ref_path, &upstream_commit)?;
            }
            refs::HeadState::Detached(_) => {
                refs::set_head_detached(&upstream_commit)?;
            }
        }

        println!("Fast-forwarded to {}.", short_hash(&upstream_commit));
        return Ok(());
    }

    let mut items: Vec<TodoItem> = Vec::new();
    for hash in collect_commits_to_replay(&our_commit, &merge_base)? {
        let subject = subject_of(&hash)?;
        items.push(TodoItem { action: Action::Pick, hash, subject });
    }

    if interactive {
        match edit_todo(&items, &merge_base, &our_commit, &upstream_commit) {
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

    begin_replay(&head_state, &our_commit, &upstream_commit, items)
}