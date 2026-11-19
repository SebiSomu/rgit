use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::Context;
use crate::helpers::checkout::{
    build_index_entries_for_tree, check_switch_safety, ensure_working_tree_clean,
    hard_reset_working_tree, sync_working_tree, update_index_from_tree,
};
use crate::helpers::commit::{
    commit_parents, extract_commit_message, resolve_tree_from_source, short_hash,
};
use crate::helpers::config::get_config_value;
use crate::helpers::merge::merge_into_worktree;
use crate::helpers::objects::{read_object, tree_hash_of_commit, write_tree_from_index_prefix};
use crate::helpers::state_files::read_lines_set;
use crate::index::{read_index, write_index};
use crate::objects::{Action, ItemResult, TodoItem};
use crate::refs;

pub const REBASE_TODO_PATH: &str = ".git/REBASE_TODO";
pub const REBASE_BRANCH_PATH: &str = ".git/REBASE_BRANCH";
pub const REBASE_ORIG_HEAD_PATH: &str = ".git/REBASE_ORIG_HEAD";
pub const REBASE_ONTO_PATH: &str = ".git/REBASE_ONTO";
pub const REBASE_PAUSE_PATH: &str = ".git/REBASE_PAUSE";
pub const REBASE_MSG_PATH: &str = ".git/REBASE_MSG";
/// Number of todo items the rebase started with (for git's `Rebasing (i/n)` progress).
pub const REBASE_TOTAL_PATH: &str = ".git/REBASE_TOTAL";
/// Present for `rebase -i`.
pub const REBASE_INTERACTIVE_PATH: &str = ".git/REBASE_INTERACTIVE";

/// Splits off the first whitespace-delimited word: `"a  b c"` -> `("a", "b c")`.
// Used by `rebase` todo parsing.
pub fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// Retrieves the subject line of a commit object.
// Used by `rebase` and `banana_split`.
pub fn subject_of(hash: &str) -> anyhow::Result<String> {
    let (object_type, content) = read_object(hash)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' is not a commit object", hash);
    }
    let message = extract_commit_message(&String::from_utf8_lossy(&content));
    Ok(message.lines().next().unwrap_or("").to_string())
}

/// Reads the stored todo (full hashes). A bare 40-hex line is read as `pick`.
// Used by `rebase`.
pub fn read_todo() -> anyhow::Result<Vec<TodoItem>> {
    let mut items = Vec::new();
    for line in read_lines_set(crate::repo::path(REBASE_TODO_PATH))? {
        let (first, rest) = split_word(&line);
        if let Some(action) = Action::parse(first) {
            let (hash, subject) = split_word(rest);
            items.push(TodoItem { action, hash: hash.to_string(), subject: subject.to_string() });
        } else if first.len() == 40 && first.chars().all(|c| c.is_ascii_hexdigit()) {
            items.push(TodoItem { action: Action::Pick, hash: first.to_string(), subject: rest.to_string() });
        } else {
            anyhow::bail!("fatal: corrupt .git/REBASE_TODO line: '{}'", line);
        }
    }
    Ok(items)
}

/// Writes the sequence of todo items to the rebase todo file.
// Used by `rebase`.
pub fn write_todo(items: &[TodoItem]) -> anyhow::Result<()> {
    if items.is_empty() {
        let _ = fs::remove_file(crate::repo::path(REBASE_TODO_PATH));
        return Ok(());
    }
    let mut content = String::new();
    for item in items {
        content.push_str(&format!("{} {} {}\n", item.action.name(), item.hash, item.subject));
    }
    fs::write(crate::repo::path(REBASE_TODO_PATH), content).context("Failed to write .git/REBASE_TODO")?;
    Ok(())
}

/// Renders the interactive todo list with help instructions for editing.
// Used by `rebase`.
pub fn render_todo_for_editing(items: &[TodoItem], base: &str, tip: &str, onto: &str) -> String {
    let mut out = String::new();
    for item in items {
        out.push_str(&format!("{} {} {}\n", item.action.name(), short_hash(&item.hash), item.subject));
    }
    out.push_str(&format!(
        "\n# Rebase {}..{} onto {} ({} command{})\n\
         #\n\
         # Commands:\n\
         # p, pick <commit>   = use commit\n\
         # r, reword <commit> = use commit, but edit the commit message\n\
         # e, edit <commit>   = use commit, but stop so you can add further commits\n\
         # s, squash <commit> = use commit, but meld into previous commit\n\
         # f, fixup <commit>  = like \"squash\", but discard this commit's message\n\
         # d, drop <commit>   = remove commit\n\
         #\n\
         # These lines can be re-ordered; they are executed from top to bottom.\n\
         # Removing a line drops that commit. Removing everything cancels the rebase.\n",
        short_hash(base),
        short_hash(tip),
        short_hash(onto),
        items.len(),
        if items.len() == 1 { "" } else { "s" }
    ));
    out
}

/// Parses the user-edited todo. Commits are matched by hash prefix against candidates.
// Used by `rebase`.
pub fn parse_edited_todo(content: &str, candidates: &[TodoItem]) -> anyhow::Result<Vec<TodoItem>> {
    parse_edited_todo_with_meld(content, candidates, false)
}

/// Parses the user-edited todo with optional permission to meld the first item into HEAD.
// Used by `rebase`.
pub fn parse_edited_todo_with_meld(content: &str, candidates: &[TodoItem], allow_first_meld: bool) -> anyhow::Result<Vec<TodoItem>> {
    let mut items: Vec<TodoItem> = Vec::new();
    let mut have_prev = allow_first_meld;

    for (n, raw) in content.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let lineno = n + 1;
        let (word, rest) = split_word(line);
        let action = Action::parse(word)
            .ok_or_else(|| anyhow::anyhow!("error: line {}: unknown command '{}'", lineno, word))?;

        let (token, _) = split_word(rest);
        if token.len() < 4 {
            anyhow::bail!("error: line {}: missing or too-short commit hash '{}'", lineno, token);
        }
        let matches: Vec<&TodoItem> = candidates.iter().filter(|c| c.hash.starts_with(token)).collect();
        let original = match matches.as_slice() {
            [one] => *one,
            [] => anyhow::bail!(
                "error: line {}: '{}' is not one of the commits being rebased",
                lineno, token
            ),
            _ => anyhow::bail!("error: line {}: '{}' is ambiguous", lineno, token),
        };
        if items.iter().any(|i| i.hash == original.hash) {
            anyhow::bail!("error: line {}: commit {} is listed twice", lineno, short_hash(&original.hash));
        }

        match action {
            Action::Squash | Action::Fixup if !have_prev => {
                anyhow::bail!("error: line {}: cannot '{}' without a previous commit", lineno, action.name());
            }
            Action::Pick | Action::Reword | Action::Edit => have_prev = true,
            _ => {}
        }

        items.push(TodoItem { action, hash: original.hash.clone(), subject: original.subject.clone() });
    }

    Ok(items)
}

/// Resolves the command string to invoke the user's configured editor.
// Used by `rebase`.
pub fn editor_command(sequence: bool) -> anyhow::Result<String> {
    let nonempty = |v: Option<String>| v.filter(|s| !s.trim().is_empty());

    if sequence {
        if let Some(e) = nonempty(std::env::var("RGIT_SEQUENCE_EDITOR").ok()) {
            return Ok(e);
        }
    }
    if let Some(e) = nonempty(std::env::var("RGIT_EDITOR").ok()) {
        return Ok(e);
    }
    if let Some(e) = nonempty(get_config_value("core.editor")?) {
        return Ok(e);
    }
    for var in ["VISUAL", "EDITOR"] {
        if let Some(e) = nonempty(std::env::var(var).ok()) {
            return Ok(e);
        }
    }
    Ok(if cfg!(windows) { "notepad" } else { "vi" }.to_string())
}

/// Launches the configured editor synchronously on the specified file.
// Used by `rebase`.
pub fn run_editor(file: &Path, sequence: bool) -> anyhow::Result<()> {
    let command = editor_command(sequence)?;
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("fatal: empty editor command"))?;
    let status = Command::new(program)
        .args(parts)
        .arg(file)
        .status()
        .with_context(|| format!("failed to launch editor '{}'", command))?;
    if !status.success() {
        anyhow::bail!("error: editor '{}' exited with {}", command, status);
    }
    Ok(())
}

/// Interactively edits the list of commits to rebase.
// Used by `rebase`.
pub fn edit_todo(items: &[TodoItem], base: &str, tip: &str, onto: &str) -> anyhow::Result<Vec<TodoItem>> {
    let path = &crate::repo::path(REBASE_TODO_PATH);
    fs::write(path, render_todo_for_editing(items, base, tip, onto))
        .context("Failed to write .git/REBASE_TODO")?;
    run_editor(path, true)?;
    let edited = fs::read_to_string(path).context("Failed to read the edited todo list")?;
    parse_edited_todo(&edited, items)
}

/// Opens an editor on `initial`, returns the message with `#` lines removed.
// Used by `rebase`.
pub fn edit_message(initial: &str) -> anyhow::Result<String> {
    let path = &crate::repo::path(REBASE_MSG_PATH);
    let mut buf = initial.trim_end().to_string();
    buf.push_str(
        "\n\n# Edit the commit message. Lines starting with '#' are ignored.\n\
         # An empty message aborts this step (retry with 'rgit rebase --continue').\n",
    );
    fs::write(path, buf).context("Failed to write .git/REBASE_MSG")?;
    run_editor(path, false)?;
    let edited = fs::read_to_string(path).context("Failed to read the edited message")?;
    let _ = fs::remove_file(path);

    let message = edited
        .trim_start_matches('\u{feff}')
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    if message.is_empty() {
        anyhow::bail!(
            "error: aborting commit due to empty commit message\n\
             hint: run 'rgit rebase --continue' to retry, or 'rgit rebase --abort'"
        );
    }
    Ok(message)
}

/// Checks whether a rebase is currently paused or in progress.
// Used by `rebase` and `banana_split`.
pub fn rebase_in_progress() -> bool {
    crate::repo::path(REBASE_TODO_PATH).exists() || crate::repo::path(REBASE_ORIG_HEAD_PATH).exists()
}

/// Replays commits onto `onto` using the rebase engine.
// Used by `banana_split`.
pub fn replay_onto(head_state: &refs::HeadState, orig_head: &str, onto: &str, commits: &[String]) -> anyhow::Result<()> {
    if rebase_in_progress() {
        anyhow::bail!(
            "fatal: a rebase is already in progress\n\
             hint: use 'rgit rebase --continue' or 'rgit rebase --abort'"
        );
    }
    let mut items = Vec::new();
    for hash in commits {
        items.push(TodoItem { action: Action::Pick, hash: hash.clone(), subject: subject_of(hash)? });
    }
    begin_replay(head_state, orig_head, onto, items, false)
}

/// git's commit selection for `rebase`: the non-merge commits of `<upstream>..<head>`, oldest
/// first in topological order, minus (unless `reapply`) those whose patch is already in
/// `<head>..<upstream>` (git's `--cherry-pick --right-only`). Returns the commits to replay
/// and the ones skipped as already applied.
// Used by `rebase`.
pub fn select_commits_to_rebase(upstream: &str, head: &str, reapply: bool) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    use crate::helpers::cherry::{commit_patch_id, walk_non_merge_commits};
    use crate::helpers::rev_list::{parse_rev_list_args, select_rev_list_commits};
    let args = vec![head.to_string(), format!("^{}", upstream), "--no-merges".to_string(), "--topo-order".to_string()];
    let (walked, _) = select_rev_list_commits(&parse_rev_list_args(&args)?)?;
    let mut ours: Vec<crate::objects::ParsedCommit> = walked.into_iter().filter(|w| !w.hidden).map(|w| w.commit).collect();
    ours.reverse();
    let mut upstream_ids = std::collections::HashSet::new();
    if !reapply {
        for commit in walk_non_merge_commits(upstream, &[head])? {
            upstream_ids.insert(commit_patch_id(&commit)?);
        }
    }
    let (mut keep, mut skipped) = (Vec::new(), Vec::new());
    for commit in &ours {
        let empty = commit_is_empty(&commit.hash)?;
        if !empty && !upstream_ids.is_empty() && upstream_ids.contains(&commit_patch_id(commit)?) {
            skipped.push(commit.hash.clone());
        } else {
            keep.push(commit.hash.clone());
        }
    }
    Ok((keep, skipped))
}

/// Whether a commit changes nothing against its first parent (or is an empty root).
// Used by `rebase` (empty commits are kept; commits that become empty are dropped).
pub fn commit_is_empty(hash: &str) -> anyhow::Result<bool> {
    let commit = crate::helpers::format_patch::parse_commit_object(hash)?;
    let parent_tree = match commit.parents.first() {
        Some(parent) => crate::helpers::format_patch::parse_commit_object(parent)?.tree,
        None => "4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_string(),
    };
    Ok(commit.tree == parent_tree)
}

/// git's `can_fast_forward`: nothing needs rewriting when `onto` is the only merge base of
/// `onto` and `head`, and also the merge base of `upstream` and `head`.
// Used by `rebase` ("Current branch <b> is up to date.").
pub fn can_fast_forward(onto: &str, upstream: &str, head: &str) -> anyhow::Result<bool> {
    use crate::helpers::merge::find_all_merge_bases;
    let bases = find_all_merge_bases(onto, head)?;
    if bases.len() != 1 || bases[0] != onto {
        return Ok(false);
    }
    let bases = find_all_merge_bases(upstream, head)?;
    Ok(bases.len() == 1 && bases[0] == onto)
}

/// Writes the rebase state, records `ORIG_HEAD`, checks out `onto` (detached) from wherever
/// `HEAD` is now, and runs the replay loop.
// Used by `rebase` and `banana_split`.
pub fn begin_replay(head_state: &refs::HeadState, orig_head: &str, onto: &str, items: Vec<TodoItem>, interactive: bool) -> anyhow::Result<()> {
    let branch_marker = match head_state {
        refs::HeadState::Branch(branch_name) => format!("refs/heads/{}", branch_name),
        refs::HeadState::Detached(_) => "HEAD".to_string(),
    };
    let current_head = refs::resolve_head_commit()?.unwrap_or_else(|| orig_head.to_string());
    fs::write(crate::repo::path(REBASE_BRANCH_PATH), format!("{}\n", branch_marker))
        .context("Failed to write .git/REBASE_BRANCH")?;
    fs::write(crate::repo::path(REBASE_ORIG_HEAD_PATH), format!("{}\n", orig_head))
        .context("Failed to write .git/REBASE_ORIG_HEAD")?;
    fs::write(crate::repo::path(REBASE_ONTO_PATH), format!("{}\n", onto))
        .context("Failed to write .git/REBASE_ONTO")?;
    fs::write(crate::repo::path(REBASE_TOTAL_PATH), format!("{}\n", items.len()))?;
    if interactive {
        fs::write(crate::repo::path(REBASE_INTERACTIVE_PATH), "")?;
    }
    fs::write(crate::repo::path(".git/ORIG_HEAD"), format!("{}\n", orig_head))?;
    write_todo(&items)?;

    let checkout = || -> anyhow::Result<()> {
        let target_tree = resolve_tree_from_source(onto)?;
        let head_tree = resolve_tree_from_source(&current_head)?;
        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> =
            index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

        check_switch_safety(&target_tree, &head_tree, &index_map)?;
        sync_working_tree(&target_tree, &head_tree, &index_map)?;
        update_index_from_tree(&target_tree, &head_tree)?;
        crate::helpers::reflog::set_reflog_action(&format!("rebase (start): checkout {}", onto));
        refs::set_head_detached(onto)?;
        Ok(())
    };
    if let Err(e) = checkout() {
        cleanup_rebase_state();
        return Err(e);
    }

    process_rebase_todo()
}

/// The raw message of a commit object (everything after the header), as stored.
fn raw_message(hash: &str) -> anyhow::Result<String> {
    let (_, content) = read_object(hash)?;
    let text = String::from_utf8_lossy(&content).to_string();
    Ok(text.split_once("\n\n").map(|(_, m)| m.to_string()).unwrap_or_default())
}

/// git's `Rebasing (i/n)` progress line (stderr, carriage return, no newline).
fn print_progress(remaining: usize) {
    let total: usize = fs::read_to_string(crate::repo::path(REBASE_TOTAL_PATH)).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(remaining);
    if total >= remaining {
        eprint!("Rebasing ({}/{})\r", total - remaining + 1, total);
    }
}

/// Continues an in-progress rebase after conflict resolution or an edit pause.
// Used by `rebase`.
pub fn rebase_continue() -> anyhow::Result<()> {
    if !crate::repo::path(REBASE_ORIG_HEAD_PATH).exists() {
        eprintln!("fatal: No rebase in progress?");
        crate::ui::exit(128);
    }

    if crate::repo::path(REBASE_PAUSE_PATH).exists() {
        ensure_working_tree_clean("continue the rebase")?;
        let _ = fs::remove_file(crate::repo::path(REBASE_PAUSE_PATH));
        return process_rebase_todo();
    }

    let todo = read_todo()?;
    let Some((item, rest)) = todo.split_first() else {
        return finish_rebase();
    };

    if crate::helpers::merge::report_needs_merge() {
        eprintln!("You must edit all merge conflicts and then");
        eprintln!("mark them as resolved using rgit add");
        crate::ui::exit(1);
    }
    let entries = read_index().unwrap_or_default();

    let current_head = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;
    let tree_hash = write_tree_from_index_prefix(&entries, "")?;

    match commit_item(item, rest, &current_head, tree_hash, true)? {
        ItemResult::Done => process_rebase_todo(),
        ItemResult::PausedForEdit => Ok(()),
    }
}

/// Aborts an in-progress rebase, resetting the working tree and HEAD to the original state.
// Used by `rebase`.
pub fn rebase_abort() -> anyhow::Result<()> {
    if !crate::repo::path(REBASE_ORIG_HEAD_PATH).exists() {
        eprintln!("fatal: No rebase in progress?");
        crate::ui::exit(128);
    }

    let orig_head = fs::read_to_string(crate::repo::path(REBASE_ORIG_HEAD_PATH))
        .context("Failed to read .git/REBASE_ORIG_HEAD")?
        .trim()
        .to_string();
    let branch_marker = fs::read_to_string(crate::repo::path(REBASE_BRANCH_PATH))
        .context("Failed to read .git/REBASE_BRANCH")?
        .trim()
        .to_string();

    let orig_tree = resolve_tree_from_source(&orig_head)?;

    let current_head = refs::resolve_head_commit()?;
    let current_tree = match &current_head {
        Some(h) => resolve_tree_from_source(h)?,
        None => BTreeMap::new(),
    };

    let index_entries = read_index().unwrap_or_default();
    let mut tracked_paths: std::collections::BTreeSet<String> =
        index_entries.iter().map(|e| e.path.clone()).collect();
    tracked_paths.extend(orig_tree.keys().cloned());
    tracked_paths.extend(current_tree.keys().cloned());

    hard_reset_working_tree(&orig_tree, &tracked_paths)?;
    let mut new_index_entries = build_index_entries_for_tree(&orig_tree, true)?;
    write_index(&mut new_index_entries)?;

    if branch_marker != "HEAD" {
        let branch_name = branch_marker.strip_prefix("refs/heads/").unwrap_or(&branch_marker).to_string();
        refs::set_head(&branch_name)?;
    } else {
        refs::set_head_detached(&orig_head)?;
    }

    cleanup_rebase_state();
    Ok(())
}

/// Skips the current commit in an in-progress rebase, resetting the working tree
/// and staging index to HEAD and continuing replay.
// Used by the `rebase` command.
pub fn rebase_skip() -> anyhow::Result<()> {
    if !crate::repo::path(REBASE_ORIG_HEAD_PATH).exists() {
        eprintln!("fatal: No rebase in progress?");
        crate::ui::exit(128);
    }

    if crate::repo::path(REBASE_PAUSE_PATH).exists() {
        let _ = fs::remove_file(crate::repo::path(REBASE_PAUSE_PATH));
        let current_head = refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD"))?;
        let (obj_type, content) = read_object(&current_head)?;
        if obj_type == "commit" {
            let parents = commit_parents(&String::from_utf8_lossy(&content));
            if let Some(parent) = parents.first() {
                refs::set_head_detached(parent)?;
                let parent_tree = resolve_tree_from_source(parent)?;
                let index_entries = read_index().unwrap_or_default();
                let mut tracked_paths: std::collections::BTreeSet<String> =
                    index_entries.iter().map(|e| e.path.clone()).collect();
                tracked_paths.extend(parent_tree.keys().cloned());
                hard_reset_working_tree(&parent_tree, &tracked_paths)?;
                let mut new_entries = build_index_entries_for_tree(&parent_tree, true)?;
                write_index(&mut new_entries)?;
                return process_rebase_todo();
            }
        }
    }

    let todo = read_todo()?;
    let current_head = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;
    let head_tree = resolve_tree_from_source(&current_head)?;

    let mut tracked_paths: std::collections::BTreeSet<String> =
        read_index().unwrap_or_default().into_iter().map(|e| e.path).collect();
    tracked_paths.extend(head_tree.keys().cloned());

    if let Some((item, rest)) = todo.split_first() {
        if let Ok(their_tree) = resolve_tree_from_source(&item.hash) {
            tracked_paths.extend(their_tree.keys().cloned());
        }
        hard_reset_working_tree(&head_tree, &tracked_paths)?;
        let mut new_index_entries = build_index_entries_for_tree(&head_tree, true)?;
        write_index(&mut new_index_entries)?;

        write_todo(rest)?;
    } else {
        hard_reset_working_tree(&head_tree, &tracked_paths)?;
        let mut new_index_entries = build_index_entries_for_tree(&head_tree, true)?;
        write_index(&mut new_index_entries)?;
    }

    process_rebase_todo()
}

/// Allows the user to view and edit the remainder of the in-progress rebase todo list.
// Used by the `rebase` command.
pub fn rebase_edit_todo() -> anyhow::Result<()> {
    if !crate::repo::path(REBASE_ORIG_HEAD_PATH).exists() {
        eprintln!("fatal: No rebase in progress?");
        crate::ui::exit(128);
    }

    let items = read_todo()?;
    if items.is_empty() {
        println!("The todo list is empty; nothing to edit.");
        return Ok(());
    }

    let onto = fs::read_to_string(crate::repo::path(REBASE_ONTO_PATH))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let orig_head = fs::read_to_string(crate::repo::path(REBASE_ORIG_HEAD_PATH))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let current_head = refs::resolve_head_commit()?
        .unwrap_or_else(|| orig_head.clone());

    let path = &crate::repo::path(REBASE_TODO_PATH);
    fs::write(path, render_todo_for_editing(&items, &current_head, &orig_head, &onto))
        .context("Failed to write .git/REBASE_TODO")?;

    run_editor(path, true)?;

    let edited = fs::read_to_string(path).context("Failed to read the edited todo list")?;
    let allow_first_meld = current_head != onto;
    match parse_edited_todo_with_meld(&edited, &items, allow_first_meld) {
        Ok(new_items) => {
            write_todo(&new_items)?;
            Ok(())
        }
        Err(e) => {
            let _ = write_todo(&items);
            Err(e)
        }
    }
}

/// Drives the replay loop against the stored todo list: git's `Rebasing (i/n)` progress,
/// fast-forwarding picks whose parent is already `HEAD`, git's conflict stop.
// Used by `rebase`.
pub fn process_rebase_todo() -> anyhow::Result<()> {
    loop {
        let todo = read_todo()?;
        let Some((item, rest)) = todo.split_first() else {
            return finish_rebase();
        };
        let item = item.clone();
        let rest = rest.to_vec();
        print_progress(todo.len());

        if item.action == Action::Drop {
            write_todo(&rest)?;
            continue;
        }

        let current_head = refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;

        let (object_type, content) = read_object(&item.hash)?;
        if object_type != "commit" {
            anyhow::bail!("fatal: '{}' does not point to a commit object", item.hash);
        }
        let text = String::from_utf8_lossy(&content).to_string();
        let parents = commit_parents(&text);
        if parents.len() > 1 {
            anyhow::bail!(
                "error: cannot rebase past merge commit {} (no support for replaying merges)",
                short_hash(&item.hash)
            );
        }

        // Like git's sequencer, a pick whose parent is HEAD needs no new commit.
        if matches!(item.action, Action::Pick | Action::Edit) && parents.first().map(String::as_str) == Some(current_head.as_str()) {
            let target = resolve_tree_from_source(&item.hash)?;
            let head_tree = resolve_tree_from_source(&current_head)?;
            let index_map: BTreeMap<String, [u8; 20]> = read_index().unwrap_or_default().iter().map(|e| (e.path.clone(), e.hash)).collect();
            sync_working_tree(&target, &head_tree, &index_map)?;
            update_index_from_tree(&target, &head_tree)?;
            crate::helpers::reflog::set_reflog_action(&format!("rebase (pick): {}", item.subject));
            refs::set_head_detached(&item.hash)?;
            write_todo(&rest)?;
            if item.action == Action::Edit {
                fs::write(crate::repo::path(REBASE_PAUSE_PATH), "edit\n").context("Failed to write .git/REBASE_PAUSE")?;
                println!("Stopped at {}... {}", short_hash(&item.hash), item.subject);
                println!("hint: add further commits if you like, then run 'rgit rebase --continue'");
                return Ok(());
            }
            continue;
        }

        let base_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
            resolve_tree_from_source(parent)?
        } else {
            BTreeMap::new()
        };
        let their_tree = resolve_tree_from_source(&item.hash)?;
        let our_tree = resolve_tree_from_source(&current_head)?;

        let their_label = format!("{} ({})", short_hash(&item.hash), item.subject);
        let result = merge_into_worktree(&base_tree, &our_tree, &their_tree, "HEAD", &their_label, "merge")?;

        if !result.conflicted.is_empty() {
            // Like git's sequencer in a rebase, the merge messages show only when a pick stops.
            for message in &result.messages {
                println!("{}", message.text);
            }
            eprintln!("error: could not apply {}... {}", short_hash(&item.hash), item.subject);
            // The narration's conflict report replaces these on a terminal.
            if !crate::ui::narration_replaces_hints() {
                eprintln!("hint: Resolve all conflicts manually, mark them as resolved with");
                eprintln!("hint: \"rgit add/rm <conflicted_files>\", then run \"rgit rebase --continue\".");
                eprintln!("hint: You can instead skip this commit: run \"rgit rebase --skip\".");
                eprintln!("hint: To abort and get back to the state before \"rgit rebase\", run \"rgit rebase --abort\".");
            }
            eprintln!("Could not apply {}... {}", short_hash(&item.hash), item.subject);
            crate::ui::exit(1);
        }

        let tree_hash = crate::helpers::state_files::build_tree_from_map(&result.tree)?;

        match commit_item(&item, &rest, &current_head, tree_hash, false)? {
            ItemResult::Done => continue,
            ItemResult::PausedForEdit => return Ok(()),
        }
    }
}

/// Applies a single todo item's commit action on top of the current head, keeping the
/// original author (the first commit's, for squash/fixup) and message. A commit that becomes
/// empty is dropped (git's `--empty=drop` and its "dropping ... -- patch contents already
/// upstream" line; `rebase -i` says "Skipping"); one that was empty to begin with is kept. With `summary`, prints git's commit summary (concluding a conflicted pick).
// Used by `rebase`.
pub fn commit_item(item: &TodoItem, rest: &[TodoItem], current_head: &str, tree_hash: String, summary: bool) -> anyhow::Result<ItemResult> {
    use crate::helpers::commit::{current_signature, print_commit_summary, write_commit_object};
    use crate::helpers::format_patch::parse_commit_object;
    let (object_type, content) = read_object(&item.hash)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' does not point to a commit object", item.hash);
    }
    let original_message = extract_commit_message(&String::from_utf8_lossy(&content))
        .trim_end()
        .to_string();
    let picked = parse_commit_object(&item.hash)?;
    let committer = current_signature()?;

    let onto = fs::read_to_string(crate::repo::path(REBASE_ONTO_PATH))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let melds = matches!(item.action, Action::Squash | Action::Fixup) && current_head != onto;

    if melds {
        let (head_type, head_content) = read_object(current_head)?;
        if head_type != "commit" {
            anyhow::bail!("fatal: HEAD '{}' is not a commit object", current_head);
        }
        let head_text = String::from_utf8_lossy(&head_content).to_string();
        let head_parents = commit_parents(&head_text);
        let [head_parent] = head_parents.as_slice() else {
            anyhow::bail!(
                "error: cannot {} into {} (not a single-parent commit)",
                item.action.name(),
                short_hash(current_head)
            );
        };
        let head_message = extract_commit_message(&head_text).trim_end().to_string();

        let mut message = if item.action == Action::Squash {
            format!("{}\n\n{}", head_message, original_message)
        } else {
            head_message
        };

        let meld_follows = matches!(
            rest.first().map(|i| i.action),
            Some(Action::Squash | Action::Fixup)
        );
        if item.action == Action::Squash && !meld_follows {
            message = edit_message(&message)?;
        }

        let head_author = parse_commit_object(current_head)?.author;
        crate::helpers::reflog::set_reflog_action(&format!("rebase ({}): {}", item.action.name(), item.subject));
        let new_commit = write_commit_object(&tree_hash, &[head_parent.clone()], &head_author, &committer, &format!("{}\n", message.trim_end()))?;
        refs::set_head_detached(&new_commit)?;
        println!(
            "[detached {}] {} ({} into previous)",
            short_hash(&new_commit),
            item.subject,
            item.action.name()
        );
        write_todo(rest)?;
        return Ok(ItemResult::Done);
    }

    if tree_hash_of_commit(current_head)? == tree_hash && !commit_is_empty(&item.hash)? {
        if crate::repo::path(REBASE_INTERACTIVE_PATH).exists() {
            println!("Skipping {}... {} (now empty)", short_hash(&item.hash), item.subject);
        } else {
            // git's `--empty=drop`.
            eprintln!("dropping {} {} -- patch contents already upstream", item.hash, item.subject);
        }
        crate::helpers::merge::clear_merge_state();
        write_todo(rest)?;
        return Ok(ItemResult::Done);
    }

    let message = if item.action == Action::Reword {
        format!("{}\n", edit_message(&original_message)?.trim_end())
    } else {
        raw_message(&item.hash)?
    };

    crate::helpers::reflog::set_reflog_action(&format!("rebase ({}): {}", item.action.name(), item.subject));
    let new_commit = write_commit_object(&tree_hash, &[current_head.to_string()], &picked.author, &committer, &message)?;
    refs::set_head_detached(&new_commit)?;
    crate::helpers::merge::clear_merge_state();
    if summary {
        print_commit_summary(&refs::HeadState::Detached(new_commit.clone()), &new_commit, &picked.author, &committer, false)?;
    } else if item.action == Action::Reword {
        println!("[detached {}] {}", short_hash(&new_commit), message.lines().next().unwrap_or(""));
    }
    write_todo(rest)?;

    if item.action == Action::Edit {
        fs::write(crate::repo::path(REBASE_PAUSE_PATH), "edit\n").context("Failed to write .git/REBASE_PAUSE")?;
        println!("Stopped at {}... {}", short_hash(&new_commit), item.subject);
        println!("hint: add further commits if you like, then run 'rgit rebase --continue'");
        return Ok(ItemResult::PausedForEdit);
    }

    Ok(ItemResult::Done)
}

/// Concludes a rebase, updating the target branch or detached HEAD.
// Used by `rebase`.
pub fn finish_rebase() -> anyhow::Result<()> {
    let final_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;

    let branch_marker = fs::read_to_string(crate::repo::path(REBASE_BRANCH_PATH))
        .context("fatal: missing .git/REBASE_BRANCH for the in-progress rebase")?
        .trim()
        .to_string();

    // git: `\r\x1b[K` clears the progress line.
    if branch_marker != "HEAD" {
        let branch_name = branch_marker.strip_prefix("refs/heads/").unwrap_or(&branch_marker).to_string();
        let onto = fs::read_to_string(crate::repo::path(REBASE_ONTO_PATH)).map(|s| s.trim().to_string()).unwrap_or_default();
        crate::helpers::reflog::set_reflog_action(&format!("rebase (finish): {} onto {}", branch_marker, onto));
        refs::write_ref(&branch_marker, &final_commit)?;
        refs::set_head(&branch_name)?;
        eprintln!("\r\x1b[KSuccessfully rebased and updated {}.", branch_marker);
    } else {
        eprintln!("\r\x1b[KSuccessfully rebased and updated detached HEAD.");
    }

    cleanup_rebase_state();

    Ok(())
}

/// Cleans up temporary rebase state files from `.git`.
// Used by `rebase`.
pub fn cleanup_rebase_state() {
    for path in [
        REBASE_TODO_PATH,
        REBASE_BRANCH_PATH,
        REBASE_ORIG_HEAD_PATH,
        REBASE_ONTO_PATH,
        REBASE_PAUSE_PATH,
        REBASE_MSG_PATH,
        REBASE_TOTAL_PATH,
        REBASE_INTERACTIVE_PATH,
    ] {
        let _ = fs::remove_file(crate::repo::path(path));
    }
}

/// Walks the commit ancestry chain from tip to base, collecting commits in replay order.
// Used by `rebase` and `banana_split`.
pub fn collect_commits_to_replay(tip: &str, base: &str) -> anyhow::Result<Vec<String>> {
    let mut commits = Vec::new();
    let mut current = tip.to_string();

    while current != base {
        let (object_type, content) = read_object(&current)?;
        if object_type != "commit" {
            anyhow::bail!("fatal: '{}' is not a commit object", current);
        }
        let text = String::from_utf8_lossy(&content).to_string();
        let parents = commit_parents(&text);

        if parents.len() > 1 {
            anyhow::bail!(
                "error: cannot rebase past merge commit {} (no support for replaying merges)",
                short_hash(&current)
            );
        }

        commits.push(current.clone());

        match parents.first() {
            Some(p) => current = p.clone(),
            None => anyhow::bail!(
                "fatal: reached root commit {} without finding the merge base — histories are unrelated",
                short_hash(&current)
            ),
        }
    }

    commits.reverse();
    Ok(commits)
}

#[cfg(test)]
#[path = "../../tests/unit/helpers/rebase.rs"]
mod tests;
