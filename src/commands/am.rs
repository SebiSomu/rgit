use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use anyhow::Result;

use crate::helpers::am::{
    advance_head, am_in_progress, build_commit_with_author, clear_am_state, head_tree_map, index_differences, parse_mail,
    read_am_message, read_am_orig_head, read_am_progress, set_am_next, split_mailbox, write_am_state, AM_STATE_DIR,
};
use crate::helpers::apply::{check_file_patch, parse_patch, read_patch_input, remove_worktree_file, write_planned_changes, write_worktree_file};
use crate::helpers::checkout::{build_index_entries_for_tree, hard_reset_working_tree};
use crate::helpers::rebase::rebase_in_progress;
use crate::helpers::reflog::set_reflog_action;
use crate::helpers::{flatten_tree, read_object, tree_hash_of_commit, write_tree_from_index_prefix};
use crate::index::{read_index, write_index, IndexEntry};
use crate::objects::{AmOptions, ApplyOptions, MailPatch, PlannedChanges};
use crate::refs;

/// `rgit am [-k] [<mbox>...]`, `rgit am (--continue | --skip | --abort | --quit | --show-current-patch[=raw|diff])`
///
/// Applies a mailbox of patches (as produced by `format-patch`, or read from standard input)
/// as commits on the current branch, keeping each mail's author, date, and message; the
/// committer is the configured identity. Each patch is applied to both index and working tree
/// all-or-nothing (as `apply --index`), so the index must match HEAD before starting. When a
/// patch does not apply, the session stops with its state in `.git/rebase-apply/`: resolve the
/// files, `rgit add` them, and `--continue`; or `--skip` the patch, `--abort` back to where `am`
/// started, or `--quit` to keep what has been applied. Exits 128 when it stops.
pub fn am(opts: AmOptions) -> Result<()> {
    match run_am(&opts) {
        Ok(true) => Ok(()),
        Ok(false) => std::process::exit(128),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(128);
        }
    }
}

fn run_am(opts: &AmOptions) -> Result<bool> {
    let resuming = opts.cont || opts.skip || opts.abort || opts.quit || opts.show_current_patch.is_some();
    if resuming {
        if !opts.mboxes.is_empty() {
            anyhow::bail!("fatal: --continue, --skip, --abort, --quit and --show-current-patch take no mailbox arguments");
        }
        if !am_in_progress() {
            anyhow::bail!("fatal: Resolve operation not in progress, we are not resuming.");
        }
    }

    if opts.quit {
        clear_am_state()?;
        return Ok(true);
    }
    if let Some(mode) = &opts.show_current_patch {
        let (next, _) = read_am_progress()?;
        let raw = read_am_message(next)?;
        match mode.as_str() {
            "raw" => print!("{}", raw),
            "diff" => print!("{}", parse_mail(&raw, opts.keep_subject)?.patch),
            other => anyhow::bail!("fatal: invalid value for --show-current-patch: {}", other),
        }
        return Ok(true);
    }
    if opts.abort {
        abort_session()?;
        return Ok(true);
    }
    if opts.skip {
        reset_index_and_worktree_to_head()?;
        let (next, _) = read_am_progress()?;
        set_am_next(next + 1)?;
        return run_session(opts);
    }
    if opts.cont {
        let (next, _) = read_am_progress()?;
        let mail = parse_mail(&read_am_message(next)?, opts.keep_subject)?;
        println!("Applying: {}", mail.subject);
        if index_differences(&read_index()?, &head_tree_map()?).is_empty() {
            println!("No changes - did you forget to use 'rgit add'?");
            println!("If there is nothing left to stage, chances are that something else");
            println!("already introduced the same changes; you might want to skip this patch.");
            print_resolve_advice();
            return Ok(false);
        }
        commit_mail(&mail)?;
        set_am_next(next + 1)?;
        return run_session(opts);
    }

    if am_in_progress() {
        if !opts.mboxes.is_empty() {
            anyhow::bail!("fatal: previous rebase directory {} still exists but mbox given.", AM_STATE_DIR);
        }
        return run_session(opts);
    }
    if rebase_in_progress() {
        anyhow::bail!("fatal: a rebase is in progress; finish it with 'rgit rebase --continue' or 'rgit rebase --abort' first");
    }

    let dirty = index_differences(&read_index()?, &head_tree_map()?);
    if !dirty.is_empty() {
        anyhow::bail!("fatal: Dirty index: cannot apply patches (dirty: {})", dirty.join(" "));
    }

    let messages = split_mailbox(&read_patch_input(&opts.mboxes)?);
    if messages.is_empty() {
        anyhow::bail!("Patch format detection failed.");
    }
    write_am_state(&messages, refs::resolve_head_commit()?.as_deref())?;
    run_session(opts)
}

fn run_session(opts: &AmOptions) -> Result<bool> {
    loop {
        let (next, last) = read_am_progress()?;
        if next > last {
            clear_am_state()?;
            return Ok(true);
        }

        let mail = parse_mail(&read_am_message(next)?, opts.keep_subject)?;
        println!("Applying: {}", mail.subject);

        let patches = parse_patch(&mail.patch, 1)?;
        if patches.is_empty() {
            println!("Patch is empty.");
            print_resolve_advice();
            return Ok(false);
        }

        let index_entries = read_index()?;
        let index_map: HashMap<String, IndexEntry> = index_entries.iter().map(|e| (e.path.clone(), e.clone())).collect();
        let apply_opts = ApplyOptions { index: true, strip: 1, ..Default::default() };
        let mut pending = PlannedChanges::new();
        let mut applies = true;
        for patch in &patches {
            if !check_file_patch(patch, &apply_opts, &index_map, &mut pending) {
                applies = false;
            }
        }
        if !applies {
            println!("Patch failed at {:04} {}", next, mail.subject);
            eprintln!("hint: Use 'rgit am --show-current-patch=diff' to see the failed patch");
            print_resolve_advice();
            return Ok(false);
        }

        write_planned_changes(&pending, false, true, index_entries)?;
        commit_mail(&mail)?;
        set_am_next(next + 1)?;
    }
}

fn commit_mail(mail: &MailPatch) -> Result<()> {
    let entries = read_index()?;
    let tree = write_tree_from_index_prefix(&entries, "")?;
    let parent = refs::resolve_head_commit()?;
    set_reflog_action(&format!("am: {}", mail.subject));
    let commit = build_commit_with_author(&tree, parent.as_deref(), &mail.author, &mail.message)?;
    advance_head(&commit)
}

fn print_resolve_advice() {
    println!("When you have resolved this problem, run \"rgit am --continue\".");
    println!("If you prefer to skip this patch, run \"rgit am --skip\" instead.");
    println!("To restore the original branch and stop patching, run \"rgit am --abort\".");
}

fn reset_index_and_worktree_to_head() -> Result<()> {
    let head = head_tree_map()?;
    for path in index_differences(&read_index()?, &head) {
        match head.get(&path) {
            Some((hash, mode)) => write_worktree_file(&path, &read_object(&hex::encode(hash))?.1, *mode)?,
            None if Path::new(&path).is_file() => remove_worktree_file(&path)?,
            None => {}
        }
    }
    let mut entries = build_index_entries_for_tree(&head, true)?;
    write_index(&mut entries)
}

fn abort_session() -> Result<()> {
    let orig = read_am_orig_head()?;
    let mut target = std::collections::BTreeMap::new();
    if let Some(orig) = &orig {
        flatten_tree(&tree_hash_of_commit(orig)?, "", &mut target)?;
    }

    let mut tracked: BTreeSet<String> = read_index()?.into_iter().map(|e| e.path).collect();
    tracked.extend(head_tree_map()?.into_keys());
    hard_reset_working_tree(&target, &tracked)?;
    let mut entries = build_index_entries_for_tree(&target, true)?;
    write_index(&mut entries)?;

    set_reflog_action("am --abort");
    match &orig {
        Some(orig) => advance_head(orig)?,
        None => {
            if let Ok(branch_ref) = refs::current_branch_ref() {
                let path = format!(".git/{}", branch_ref);
                if Path::new(&path).exists() {
                    fs::remove_file(path)?;
                }
            }
        }
    }
    clear_am_state()
}
