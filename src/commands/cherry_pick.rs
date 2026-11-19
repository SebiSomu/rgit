use std::collections::BTreeMap;
use std::fs;
use crate::core::commit::{current_signature, print_commit_summary, write_commit_object};
use crate::helpers::format_patch::parse_commit_object;
use crate::helpers::merge::{clear_merge_state, die_if_unmerged, merge_into_worktree, reset_merge_to_tree};
use crate::helpers::sequencer::{
    append_cherry_picked_from, append_signoff, read_sequence_options, read_todo, remove_sequence, rollback_is_safe,
    save_todo, sequence_head, sequencer_active, start_sequence, update_abort_safety,
};
use crate::helpers::state_files::build_tree_from_map;
use crate::core::commit::resolve_tree_from_source;
use crate::objects::CherryPickOptions;
use crate::core::refs;

type TreeMap = BTreeMap<String, ([u8; 20], u32)>;

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// How one pick ended: applied (committed, or staged with `-n`), or stopped with an exit code
/// after printing why (conflict, empty result, or an error).
enum Pick {
    Done,
    Stop(i32),
}

fn fail(message: &str) -> Pick {
    eprintln!("{}", message);
    eprintln!("fatal: cherry-pick failed");
    Pick::Stop(128)
}

/// `rgit cherry-pick [<options>] <commit>...` / `--continue` / `--skip` / `--abort` / `--quit`
///
/// Replays each commit's changes (against its parent, or the `-m <n>`-th parent of a merge)
/// onto `HEAD` with git's line-level three-way merge, labelled `HEAD` and `<abbrev> (<subject>)`.
/// Commits are picked in the order given; with a range (`A..B`, `^X`) they are walked like
/// `rev-list --reverse`, oldest first. Each clean pick is committed with the original author
/// and message (`-x` adds `(cherry picked from commit <hash>)`, `-s` a `Signed-off-by:`) and
/// git's summary; `-n` only updates the index and working tree, and `--ff` fast-forwards
/// when `HEAD` is the commit's parent. A pick whose result changes nothing stops (unless
/// `--keep-redundant-commits`, or `--allow-empty` for commits that were empty to begin with).
/// On conflicts the index holds stages 1-3 and `CHERRY_PICK_HEAD`/`MERGE_MSG` are written;
/// with several commits the rest of the sequence waits in `.git/sequencer`. `--continue`
/// commits the resolved pick and goes on, `--skip` drops it, `--abort` goes back to where the
/// sequence started, and `--quit` forgets the sequence and keeps everything as it is.
/// Messages, streams, state files and exit codes follow git 2.43.
pub fn cherry_pick(opts: CherryPickOptions) -> anyhow::Result<()> {
    let action = if opts.cont { Some("--continue") } else if opts.skip { Some("--skip") } else if opts.abort { Some("--abort") } else if opts.quit { Some("--quit") } else { None };
    if let Some(flag) = action {
        if !opts.commits.is_empty() {
            eprintln!("fatal: cherry-pick: {} takes no commit arguments", flag);
            crate::ui::exit(129);
        }
    }
    let outcome = if opts.quit {
        remove_sequence();
        let _ = fs::remove_file(crate::core::repo::path(".git/CHERRY_PICK_HEAD"));
        clear_merge_state();
        Pick::Done
    } else if opts.abort {
        abort()?
    } else if opts.skip {
        skip()?
    } else if opts.cont {
        resume()?
    } else {
        start(&opts)?
    };
    match outcome {
        Pick::Done => Ok(()),
        Pick::Stop(code) => crate::ui::exit(code),
    }
}

fn start(opts: &CherryPickOptions) -> anyhow::Result<Pick> {
    if opts.commits.is_empty() {
        eprintln!("usage: rgit cherry-pick [-n] [-m <parent-number>] [-s] [-x] [--ff] <commit>...");
        eprintln!("   or: rgit cherry-pick (--continue | --skip | --abort | --quit)");
        return Ok(Pick::Stop(129));
    }
    let walk = opts.commits.iter().any(|c| c.contains("..") || c.starts_with('^'));
    let commits = if walk {
        let args = opts.commits.clone();
        let rev_opts = match crate::helpers::rev_list::parse_rev_list_args(&args) {
            Ok(rev_opts) => rev_opts,
            Err(_) => {
                let bad = opts.commits.iter().find(|c| crate::helpers::rev_list::parse_rev_list_args(&[c.to_string()]).is_err()).cloned().unwrap_or_default();
                eprintln!("fatal: bad revision '{}'", bad);
                return Ok(Pick::Stop(128));
            }
        };
        let (walked, _) = crate::helpers::rev_list::select_rev_list_commits(&rev_opts)?;
        // Oldest first, like `rev-list --reverse`.
        walked.iter().filter(|c| !c.hidden).rev().map(|c| c.commit.hash.clone()).collect::<Vec<_>>()
    } else {
        let mut commits: Vec<String> = Vec::new();
        for rev in &opts.commits {
            match crate::core::commit::resolve_commit_from_source(rev) {
                Ok(hash) if !commits.contains(&hash) => commits.push(hash),
                Ok(_) => {}
                Err(_) => {
                    eprintln!("fatal: bad revision '{}'", rev);
                    return Ok(Pick::Stop(128));
                }
            }
        }
        commits
    };
    if commits.is_empty() {
        return Ok(fail("error: empty commit set passed"));
    }

    // One plain revision is a single pick: no sequencer.
    if opts.commits.len() == 1 && !walk {
        return pick_one(&commits[0], opts);
    }
    if sequencer_active() {
        eprintln!("error: cherry-pick is already in progress");
        let skip = if crate::core::repo::path(".git/CHERRY_PICK_HEAD").exists() { "--skip | " } else { "" };
        eprintln!("hint: try \"rgit cherry-pick (--continue | {}--abort | --quit)\"", skip);
        eprintln!("fatal: cherry-pick failed");
        return Ok(Pick::Stop(128));
    }
    let head = refs::resolve_head_commit()?.unwrap_or_default();
    start_sequence(&head, opts)?;
    run_sequence(&commits, opts)
}

/// Picks `commits` in order, keeping the todo list current; the sequence ends when all are in.
fn run_sequence(commits: &[String], opts: &CherryPickOptions) -> anyhow::Result<Pick> {
    for i in 0..commits.len() {
        save_todo(&commits[i..])?;
        let outcome = pick_one(&commits[i], opts)?;
        // git's do_pick_commit always ends by recording where HEAD is.
        update_abort_safety(&refs::resolve_head_commit()?.unwrap_or_default())?;
        if let Pick::Stop(code) = outcome {
            return Ok(Pick::Stop(code));
        }
    }
    remove_sequence();
    Ok(Pick::Done)
}

/// `HEAD`'s commit and tree (an unborn branch has neither).
fn head_tree() -> anyhow::Result<(Option<String>, TreeMap)> {
    let head = refs::resolve_head_commit()?;
    let tree = match &head {
        Some(commit) => resolve_tree_from_source(commit)?,
        None => TreeMap::new(),
    };
    Ok((head, tree))
}

/// The index as a tree map, or `None` when it has unmerged entries.
fn index_tree() -> Option<TreeMap> {
    let entries = crate::core::index::read_index().unwrap_or_default();
    if entries.iter().any(|e| e.stage != 0) {
        return None;
    }
    Some(entries.iter().map(|e| (e.path.clone(), (e.hash, e.mode))).collect())
}

fn dirty_index_error() -> anyhow::Result<Pick> {
    die_if_unmerged("Cherry-picking", "cherry-pick failed")?;
    eprintln!("error: your local changes would be overwritten by cherry-pick.");
    eprintln!("hint: commit your changes or stash them to proceed.");
    eprintln!("fatal: cherry-pick failed");
    Ok(Pick::Stop(128))
}

/// git's `do_pick_commit` for one commit.
fn pick_one(commit: &str, opts: &CherryPickOptions) -> anyhow::Result<Pick> {
    die_if_unmerged("Cherry-picking", "cherry-pick failed")?;
    let head_state = refs::resolve_head()?;
    let (head, head_map) = head_tree()?;
    // `-n` merges into whatever the index holds; otherwise the index must match HEAD.
    let our_map = if opts.no_commit {
        match index_tree() {
            Some(tree) => tree,
            None => return Ok(fail("error: your index file is unmerged.")),
        }
    } else {
        if index_tree().as_ref() != Some(&head_map) {
            return dirty_index_error();
        }
        head_map.clone()
    };

    let picked = parse_commit_object(commit)?;
    let parent = match (picked.parents.len(), opts.mainline) {
        (n, None) if n > 1 => return Ok(fail(&format!("error: commit {} is a merge but no -m option was given.", commit))),
        (n, Some(m)) if m == 0 || m > n => return Ok(fail(&format!("error: commit {} does not have parent {}", commit, m))),
        (_, Some(m)) => Some(picked.parents[m - 1].clone()),
        (0, None) => None,
        (_, None) => Some(picked.parents[0].clone()),
    };

    // `--ff`: HEAD is already the parent, so the commit itself becomes HEAD.
    if opts.ff && !opts.no_commit && parent.as_deref() == head.as_deref() {
        let target = resolve_tree_from_source(commit)?;
        let index_map: BTreeMap<String, [u8; 20]> = head_map.iter().map(|(p, (h, _))| (p.clone(), *h)).collect();
        if let Err(e) = crate::helpers::checkout::check_switch_safety(&target, &head_map, &index_map) {
            return Ok(fail(&e.to_string()));
        }
        crate::helpers::checkout::sync_working_tree(&target, &head_map, &index_map)?;
        crate::helpers::checkout::update_index_from_tree(&target, &head_map)?;
        crate::core::reflog::set_reflog_action(&format!("cherry-pick: fast-forward"));
        move_head(&head_state, commit)?;
        update_abort_safety(commit)?;
        return Ok(Pick::Done);
    }

    let raw = String::from_utf8_lossy(&crate::core::object_store::read_object(commit)?.1).to_string();
    let mut message = crate::core::commit::extract_commit_message(&raw);
    if !message.ends_with('\n') {
        message.push('\n');
    }
    let committer = current_signature()?;
    if opts.record_origin {
        message = append_cherry_picked_from(&message, commit);
    }
    if opts.signoff {
        message = append_signoff(&message, &committer);
    }
    let subject = picked.message.lines().next().unwrap_or("").to_string();
    let abbrev = crate::helpers::rev_list::abbreviate_hash(commit);
    let label = format!("{} ({})", abbrev, subject);

    let base_map = match &parent {
        Some(p) => resolve_tree_from_source(p)?,
        None => TreeMap::new(),
    };
    let their_map = resolve_tree_from_source(commit)?;
    let result = match merge_into_worktree(&base_map, &our_map, &their_map, "HEAD", &label, "merge") {
        Ok(result) => result,
        Err(e) => return Ok(fail(&e.to_string())),
    };
    for line in &result.messages {
        println!("{}", line.text);
    }
    // git's sequencer records every merge result, clean or not, and leaves it behind.
    fs::write(crate::core::repo::path(".git/AUTO_MERGE"), format!("{}\n", build_tree_from_map(&result.tree)?))?;
    let merge_msg = crate::core::repo::path(".git/MERGE_MSG");
    let pick_head = crate::core::repo::path(".git/CHERRY_PICK_HEAD");

    if !result.conflicted.is_empty() {
        let mut text = format!("{}\n# Conflicts:\n", message);
        for path in result.conflicted.keys() {
            text.push_str(&format!("#\t{}\n", path));
        }
        fs::write(&merge_msg, text)?;
        eprintln!("error: could not apply {}... {}", abbrev, subject);
        if opts.no_commit {
            // The narration's conflict report replaces these on a terminal.
            if !crate::ui::narration_replaces_hints() {
                eprintln!("hint: after resolving the conflicts, mark the corrected paths");
                eprintln!("hint: with 'rgit add <paths>' or 'rgit rm <paths>'");
            }
        } else {
            fs::write(&pick_head, format!("{}\n", commit))?;
            // The narration's conflict report replaces these on a terminal.
            if !crate::ui::narration_replaces_hints() {
                eprintln!("hint: After resolving the conflicts, mark them with");
                eprintln!("hint: \"rgit add/rm <pathspec>\", then run");
                eprintln!("hint: \"rgit cherry-pick --continue\".");
                eprintln!("hint: You can instead skip this commit with \"rgit cherry-pick --skip\".");
                eprintln!("hint: To abort and get back to the state before \"rgit cherry-pick\",");
                eprintln!("hint: run \"rgit cherry-pick --abort\".");
            }
        }
        return Ok(Pick::Stop(1));
    }
    fs::write(&merge_msg, &message)?;
    if opts.no_commit {
        return Ok(Pick::Done);
    }

    let tree = build_tree_from_map(&result.tree)?;
    let head_tree_hash = match &head {
        Some(h) => parse_commit_object(h)?.tree,
        None => EMPTY_TREE.to_string(),
    };
    if tree == head_tree_hash {
        let originally_empty = picked.tree == match &parent {
            Some(p) => parse_commit_object(p)?.tree,
            None => EMPTY_TREE.to_string(),
        };
        let keep = opts.keep_redundant || (originally_empty && opts.allow_empty);
        if !keep {
            fs::write(&pick_head, format!("{}\n", commit))?;
            crate::commands::staging::status(crate::objects::StatusOptions { untracked: "normal".to_string(), ..Default::default() })?;
            eprintln!("The previous cherry-pick is now empty, possibly due to conflict resolution.");
            eprintln!("If you wish to commit it anyway, use:");
            eprintln!();
            eprintln!("    rgit commit --allow-empty");
            eprintln!();
            eprintln!("Otherwise, please use 'rgit cherry-pick --skip'");
            return Ok(Pick::Stop(1));
        }
    }
    if message.trim().is_empty() && !opts.allow_empty_message {
        return Ok(fail("Aborting commit due to empty commit message."));
    }

    let parents: Vec<String> = head.iter().cloned().collect();
    let subject_line = crate::helpers::format_patch::split_commit_message(&message).0;
    crate::core::reflog::set_reflog_action(&format!("cherry-pick: {}", subject_line));
    let new_commit = write_commit_object(&tree, &parents, &picked.author, &committer, &message)?;
    move_head(&head_state, &new_commit)?;
    let _ = fs::remove_file(&merge_msg);
    let _ = fs::remove_file(&pick_head);
    update_abort_safety(&new_commit)?;
    print_commit_summary(&head_state, &new_commit, &picked.author, &committer, true)?;
    Ok(Pick::Done)
}

fn move_head(head_state: &refs::HeadState, commit: &str) -> anyhow::Result<()> {
    match head_state {
        refs::HeadState::Branch(branch) => refs::write_ref(&format!("refs/heads/{}", branch), commit),
        refs::HeadState::Detached(_) => refs::set_head_detached(commit),
    }
}

/// Commits the resolved pick the way git's `continue_single_pick` does: `commit --no-edit
/// --cleanup=strip` (`MERGE_MSG` without its `# Conflicts:` comment, the picked commit's author).
fn commit_resolved_pick() -> anyhow::Result<()> {
    crate::commands::commit::commit(crate::objects::CommitOptions { no_edit: true, cleanup_strip: true, ..Default::default() })
}

/// `--continue`.
fn resume() -> anyhow::Result<Pick> {
    let pick_head = crate::core::repo::path(".git/CHERRY_PICK_HEAD");
    if !sequencer_active() {
        if !pick_head.exists() {
            return Ok(fail("error: no cherry-pick or revert in progress"));
        }
        commit_resolved_pick()?;
        return Ok(Pick::Done);
    }
    let opts = read_sequence_options();
    let todo = read_todo()?;
    if pick_head.exists() {
        commit_resolved_pick()?;
    }
    continue_sequence(&todo, &opts)
}

/// Drops the current todo entry and picks the rest (git's `current++`), after checking the
/// index matches `HEAD`.
fn continue_sequence(todo: &[String], opts: &CherryPickOptions) -> anyhow::Result<Pick> {
    if !sequencer_active() {
        // The commit that concluded the last pick ended the sequence.
        return Ok(Pick::Done);
    }
    let (_, head_map) = head_tree()?;
    if index_tree().as_ref() != Some(&head_map) {
        return dirty_index_error();
    }
    run_sequence(todo.get(1..).unwrap_or(&[]), opts)
}

/// Writes `ORIG_HEAD` and resets index and working tree to `target` like `reset --merge`,
/// moving the branch there; all merge and pick state goes.
fn reset_merge(target: &str) -> anyhow::Result<()> {
    let head_state = refs::resolve_head()?;
    if let Some(head) = refs::resolve_head_commit()? {
        fs::write(crate::core::repo::path(".git/ORIG_HEAD"), format!("{}\n", head))?;
        if head != target {
            crate::core::reflog::set_reflog_action(&format!("reset: moving to {}", target));
        }
    }
    let target_tree = if target.is_empty() { TreeMap::new() } else { resolve_tree_from_source(target)? };
    reset_merge_to_tree(&target_tree)?;
    if !target.is_empty() {
        move_head(&head_state, target)?;
    }
    clear_merge_state();
    let _ = fs::remove_file(crate::core::repo::path(".git/CHERRY_PICK_HEAD"));
    Ok(())
}

/// `--skip`.
fn skip() -> anyhow::Result<Pick> {
    // Without CHERRY_PICK_HEAD there is something to skip only mid-sequence, and only while
    // HEAD has not moved (otherwise the pick was committed by hand).
    let head = refs::resolve_head_commit()?.unwrap_or_default();
    if !crate::core::repo::path(".git/CHERRY_PICK_HEAD").exists() {
        if !sequencer_active() {
            return Ok(fail("error: no cherry-pick in progress"));
        }
        if !rollback_is_safe(&head) {
            eprintln!("error: there is nothing to skip");
            eprintln!("hint: have you committed already?");
            eprintln!("hint: try \"rgit cherry-pick --continue\"");
            eprintln!("fatal: cherry-pick failed");
            return Ok(Pick::Stop(128));
        }
    }
    reset_merge(&head)?;
    if !sequencer_active() {
        return Ok(Pick::Done);
    }
    let opts = read_sequence_options();
    let todo = read_todo()?;
    continue_sequence(&todo, &opts)
}

/// `--abort`.
fn abort() -> anyhow::Result<Pick> {
    if !sequencer_active() {
        if !crate::core::repo::path(".git/CHERRY_PICK_HEAD").exists() {
            return Ok(fail("error: no cherry-pick or revert in progress"));
        }
        let head = refs::resolve_head_commit()?.unwrap_or_default();
        reset_merge(&head)?;
        return Ok(Pick::Done);
    }
    let current = refs::resolve_head_commit()?.unwrap_or_default();
    if !rollback_is_safe(&current) {
        eprintln!("warning: You seem to have moved HEAD. Not rewinding, check your HEAD!");
    } else {
        let start = sequence_head().unwrap_or_default();
        reset_merge(&start)?;
    }
    remove_sequence();
    Ok(Pick::Done)
}
