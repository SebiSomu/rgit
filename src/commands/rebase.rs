use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;
use anyhow::Context;
use crate::commands::{build_index_entries_for_tree, hard_reset_working_tree};
use crate::commands::commit::build_commit;
use crate::commands::plumbing::write_tree_from_index_prefix;
use crate::helpers::checkout::{check_switch_safety, ensure_working_tree_clean, sync_working_tree, update_index_from_tree};
use crate::helpers::config::get_config_value;
use crate::helpers::state_files::read_lines_set;
use crate::helpers::{commit_parents, extract_commit_message, find_merge_base, generate_conflict_markers, read_object, resolve_commit_from_source, resolve_tree_from_source, three_way_tree_merge, tree_hash_of_commit};
use crate::index::{build_entry, read_index, write_index};
use crate::refs;

const REBASE_TODO_PATH: &str = ".git/REBASE_TODO";
const REBASE_BRANCH_PATH: &str = ".git/REBASE_BRANCH";
const REBASE_ORIG_HEAD_PATH: &str = ".git/REBASE_ORIG_HEAD";
const REBASE_ONTO_PATH: &str = ".git/REBASE_ONTO";
const REBASE_PAUSE_PATH: &str = ".git/REBASE_PAUSE";
const REBASE_MSG_PATH: &str = ".git/REBASE_MSG";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

impl Action {
    fn parse(word: &str) -> Option<Action> {
        match word {
            "pick" | "p" => Some(Action::Pick),
            "reword" | "r" => Some(Action::Reword),
            "edit" | "e" => Some(Action::Edit),
            "squash" | "s" => Some(Action::Squash),
            "fixup" | "f" => Some(Action::Fixup),
            "drop" | "d" => Some(Action::Drop),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Action::Pick => "pick",
            Action::Reword => "reword",
            Action::Edit => "edit",
            Action::Squash => "squash",
            Action::Fixup => "fixup",
            Action::Drop => "drop",
        }
    }
}

#[derive(Clone, Debug)]
struct TodoItem {
    action: Action,
    hash: String,
    subject: String,
}

enum ItemResult {
    Done,
    PausedForEdit,
}

/// Splits off the first whitespace-delimited word: `"a  b c"` -> `("a", "b c")`.
fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn subject_of(hash: &str) -> anyhow::Result<String> {
    let (object_type, content) = read_object(hash)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' is not a commit object", hash);
    }
    let message = extract_commit_message(&String::from_utf8_lossy(&content));
    Ok(message.lines().next().unwrap_or("").to_string())
}

/// Reads the stored todo (full hashes). A bare 40-hex line is read as `pick`.
fn read_todo() -> anyhow::Result<Vec<TodoItem>> {
    let mut items = Vec::new();
    for line in read_lines_set(REBASE_TODO_PATH)? {
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

fn write_todo(items: &[TodoItem]) -> anyhow::Result<()> {
    if items.is_empty() {
        let _ = fs::remove_file(REBASE_TODO_PATH);
        return Ok(());
    }
    let mut content = String::new();
    for item in items {
        content.push_str(&format!("{} {} {}\n", item.action.name(), item.hash, item.subject));
    }
    fs::write(REBASE_TODO_PATH, content).context("Failed to write .git/REBASE_TODO")?;
    Ok(())
}

fn render_todo_for_editing(items: &[TodoItem], base: &str, tip: &str, onto: &str) -> String {
    let mut out = String::new();
    for item in items {
        out.push_str(&format!("{} {} {}\n", item.action.name(), short(&item.hash), item.subject));
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
        short(base),
        short(tip),
        short(onto),
        items.len(),
        if items.len() == 1 { "" } else { "s" }
    ));
    out
}

/// Parses the user-edited todo. Commits are matched by hash prefix against
/// `candidates` (the commits actually being rebased), so abbreviated hashes
/// work and foreign hashes are rejected.
fn parse_edited_todo(content: &str, candidates: &[TodoItem]) -> anyhow::Result<Vec<TodoItem>> {
    let mut items: Vec<TodoItem> = Vec::new();
    let mut have_prev = false;

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
            anyhow::bail!("error: line {}: commit {} is listed twice", lineno, short(&original.hash));
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

fn editor_command(sequence: bool) -> anyhow::Result<String> {
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

fn run_editor(file: &Path, sequence: bool) -> anyhow::Result<()> {
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

fn edit_todo(items: &[TodoItem], base: &str, tip: &str, onto: &str) -> anyhow::Result<Vec<TodoItem>> {
    let path = Path::new(REBASE_TODO_PATH);
    fs::write(path, render_todo_for_editing(items, base, tip, onto))
        .context("Failed to write .git/REBASE_TODO")?;
    run_editor(path, true)?;
    let edited = fs::read_to_string(path).context("Failed to read the edited todo list")?;
    parse_edited_todo(&edited, items)
}

/// Opens an editor on `initial`, returns the message with `#` lines removed.
/// An empty result is an error; the rebase stays paused so that fixing the
/// message and running `--continue` retries the same commit.
fn edit_message(initial: &str) -> anyhow::Result<String> {
    let path = Path::new(REBASE_MSG_PATH);
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

/// `rgit rebase [-i] <upstream>` / `--continue` / `--abort`
pub fn rebase(upstream: Option<String>, interactive: bool, cont: bool, abort: bool) -> anyhow::Result<()> {
    if abort {
        return rebase_abort();
    }
    if cont {
        return rebase_continue();
    }

    let upstream = upstream.ok_or_else(|| {
        anyhow::anyhow!("fatal: rebase requires an <upstream>, or --continue / --abort")
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

        println!("Fast-forwarded to {}.", short(&upstream_commit));
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

/// True while a rebase (or anything built on the rebase engine) is paused.
pub(crate) fn rebase_in_progress() -> bool {
    Path::new(REBASE_TODO_PATH).exists() || Path::new(REBASE_ORIG_HEAD_PATH).exists()
}

/// Replays `commits` (oldest first, as plain picks) on top of `onto`, then
/// moves the current branch to the result. This is the rebase engine's entry
/// point for other commands (e.g. `banana-split`): conflicts pause it exactly
/// like a rebase, and `rgit rebase --continue` / `--abort` take over.
/// `orig_head` is the tip being replaced, kept so `--abort` can restore it.
pub(crate) fn replay_onto(head_state: &refs::HeadState, orig_head: &str, onto: &str, commits: &[String]) -> anyhow::Result<()> {
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
    begin_replay(head_state, orig_head, onto, items)
}

/// Writes the rebase state, checks out `onto`, and runs the replay loop.
fn begin_replay(head_state: &refs::HeadState, orig_head: &str, onto: &str, items: Vec<TodoItem>) -> anyhow::Result<()> {
    let branch_marker = match head_state {
        refs::HeadState::Branch(branch_name) => format!("refs/heads/{}", branch_name),
        refs::HeadState::Detached(_) => "HEAD".to_string(),
    };
    fs::write(REBASE_BRANCH_PATH, format!("{}\n", branch_marker))
        .context("Failed to write .git/REBASE_BRANCH")?;
    fs::write(REBASE_ORIG_HEAD_PATH, format!("{}\n", orig_head))
        .context("Failed to write .git/REBASE_ORIG_HEAD")?;
    fs::write(REBASE_ONTO_PATH, format!("{}\n", onto))
        .context("Failed to write .git/REBASE_ONTO")?;
    write_todo(&items)?;

    let checkout = || -> anyhow::Result<()> {
        let target_tree = resolve_tree_from_source(onto)?;
        let head_tree = resolve_tree_from_source(orig_head)?;
        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> =
            index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

        check_switch_safety(&target_tree, &head_tree, &index_map)?;
        sync_working_tree(&target_tree, &head_tree, &index_map)?;
        update_index_from_tree(&target_tree, &head_tree)?;
        refs::set_head_detached(onto)?;
        Ok(())
    };
    if let Err(e) = checkout() {
        cleanup_rebase_state();
        return Err(e);
    }

    println!("Rebasing {} commit(s) onto {}...", items.len(), short(onto));

    process_rebase_todo()
}

/// `rgit rebase --continue`
fn rebase_continue() -> anyhow::Result<()> {
    if !Path::new(REBASE_ORIG_HEAD_PATH).exists() {
        anyhow::bail!("fatal: no rebase in progress");
    }

    if Path::new(REBASE_PAUSE_PATH).exists() {
        ensure_working_tree_clean("continue the rebase")?;
        let _ = fs::remove_file(REBASE_PAUSE_PATH);
        return process_rebase_todo();
    }

    let todo = read_todo()?;
    let Some((item, rest)) = todo.split_first() else {
        return finish_rebase();
    };

    let entries = read_index().unwrap_or_default();
    for entry in &entries {
        if let Ok(content) = fs::read(&entry.path) {
            if content.windows(7).any(|w| w == b"<<<<<<<") {
                anyhow::bail!(
                    "error: '{}' still has unresolved conflict markers; fix it and 'rgit add' it first",
                    entry.path
                );
            }
        }
    }

    let current_head = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;
    let tree_hash = write_tree_from_index_prefix(&entries, "")?;

    match commit_item(item, rest, &current_head, tree_hash)? {
        ItemResult::Done => process_rebase_todo(),
        ItemResult::PausedForEdit => Ok(()),
    }
}

/// `rgit rebase --abort`
fn rebase_abort() -> anyhow::Result<()> {
    if !Path::new(REBASE_ORIG_HEAD_PATH).exists() {
        anyhow::bail!("fatal: no rebase in progress");
    }

    let orig_head = fs::read_to_string(REBASE_ORIG_HEAD_PATH)
        .context("Failed to read .git/REBASE_ORIG_HEAD")?
        .trim()
        .to_string();
    let branch_marker = fs::read_to_string(REBASE_BRANCH_PATH)
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

    println!("Rebase aborted; HEAD left at {}.", short(&orig_head));

    Ok(())
}

/// Drives the replay against the stored todo, one item at a time, until the
/// list empties (success) or something pauses it (conflict, empty message,
/// `edit`).
fn process_rebase_todo() -> anyhow::Result<()> {
    loop {
        let todo = read_todo()?;
        let Some((item, rest)) = todo.split_first() else {
            return finish_rebase();
        };
        let item = item.clone();
        let rest = rest.to_vec();

        if item.action == Action::Drop {
            println!("Dropped {}... {}", short(&item.hash), item.subject);
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
                short(&item.hash)
            );
        }

        let base_tree: BTreeMap<String, ([u8; 20], u32)> = if let Some(parent) = parents.first() {
            resolve_tree_from_source(parent)?
        } else {
            BTreeMap::new()
        };
        let their_tree = resolve_tree_from_source(&item.hash)?;
        let our_tree = resolve_tree_from_source(&current_head)?;

        let (merged_tree, conflicts) = three_way_tree_merge(&base_tree, &our_tree, &their_tree)?;

        if !conflicts.is_empty() {
            for conflict in &conflicts {
                let conflict_content = generate_conflict_markers(
                    &conflict.ours,
                    &conflict.theirs,
                    &format!("{}...", short(&item.hash)),
                );
                if let Some(parent) = Path::new(&conflict.path).parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)?;
                    }
                }
                fs::write(&conflict.path, &conflict_content)?;
                println!("CONFLICT (content): Merge conflict in {}", conflict.path);
            }

            for (path, (hash, _mode)) in &merged_tree {
                let (_, content) = read_object(&hex::encode(hash))?;
                if let Some(parent) = Path::new(path).parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent)?;
                    }
                }
                fs::write(path, &content)?;
            }

            println!("error: could not apply {}... {}", short(&item.hash), item.subject);
            println!("hint: after resolving the conflicts, mark the corrected paths");
            println!("hint: with 'rgit add <paths>' and run 'rgit rebase --continue'");
            println!("hint: (or 'rgit rebase --abort' to give up)");
            return Ok(());
        }

        for path in our_tree.keys() {
            if !merged_tree.contains_key(path) {
                let path_obj = Path::new(path);
                if path_obj.exists() {
                    let _ = fs::remove_file(path_obj);
                }
            }
        }

        let mut new_index_entries = Vec::new();
        for (path, (hash, _mode)) in &merged_tree {
            let (_, content) = read_object(&hex::encode(hash))?;
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(path, &content)?;

            let metadata = fs::metadata(path)?;
            let entry = build_entry(path, *hash, &metadata);
            new_index_entries.push(entry);
        }
        write_index(&mut new_index_entries)?;

        let tree_hash = write_tree_from_index_prefix(&new_index_entries, "")?;

        match commit_item(&item, &rest, &current_head, tree_hash)? {
            ItemResult::Done => continue,
            ItemResult::PausedForEdit => return Ok(()),
        }
    }
}

/// Turns the staged result of applying `item` on top of `current_head`
/// (`tree_hash`) into a commit according to the item's action, then pops it
/// from the todo (`rest` is what remains after it).
///
/// Shared by the replay loop (clean merge) and `--continue` (conflict
/// resolved / message retry), so both behave identically.
fn commit_item(item: &TodoItem, rest: &[TodoItem], current_head: &str, tree_hash: String) -> anyhow::Result<ItemResult> {
    let (object_type, content) = read_object(&item.hash)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' does not point to a commit object", item.hash);
    }
    let original_message = extract_commit_message(&String::from_utf8_lossy(&content))
        .trim_end()
        .to_string();

    let onto = fs::read_to_string(REBASE_ONTO_PATH)
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
                short(current_head)
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

        let new_commit = build_commit(tree_hash, Some(head_parent.clone()), &message)?;
        refs::set_head_detached(&new_commit)?;
        println!(
            "[detached {}] {} ({} into previous)",
            short(&new_commit),
            item.subject,
            item.action.name()
        );
        write_todo(rest)?;
        return Ok(ItemResult::Done);
    }

    if tree_hash_of_commit(current_head)? == tree_hash {
        println!("Skipping {}... {} (now empty)", short(&item.hash), item.subject);
        write_todo(rest)?;
        return Ok(ItemResult::Done);
    }

    let message = if item.action == Action::Reword {
        edit_message(&original_message)?
    } else {
        original_message
    };

    let new_commit = build_commit(tree_hash, Some(current_head.to_string()), &message)?;
    refs::set_head_detached(&new_commit)?;
    println!("[detached {}] {}", short(&new_commit), message.lines().next().unwrap_or(""));
    write_todo(rest)?;

    if item.action == Action::Edit {
        fs::write(REBASE_PAUSE_PATH, "edit\n").context("Failed to write .git/REBASE_PAUSE")?;
        println!("Stopped at {}... {}", short(&new_commit), item.subject);
        println!("hint: add further commits if you like, then run 'rgit rebase --continue'");
        return Ok(ItemResult::PausedForEdit);
    }

    Ok(ItemResult::Done)
}

fn finish_rebase() -> anyhow::Result<()> {
    let final_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: rebase lost HEAD -- this should not happen"))?;

    let branch_marker = fs::read_to_string(REBASE_BRANCH_PATH)
        .context("fatal: missing .git/REBASE_BRANCH for the in-progress rebase")?
        .trim()
        .to_string();

    if branch_marker != "HEAD" {
        let branch_name = branch_marker.strip_prefix("refs/heads/").unwrap_or(&branch_marker).to_string();
        refs::write_ref(&branch_marker, &final_commit)?;
        refs::set_head(&branch_name)?;
        println!("Successfully rebased and updated {}.", branch_marker);
    } else {
        println!("Successfully rebased onto {} (detached HEAD).", short(&final_commit));
    }

    cleanup_rebase_state();

    Ok(())
}

fn cleanup_rebase_state() {
    for path in [
        REBASE_TODO_PATH,
        REBASE_BRANCH_PATH,
        REBASE_ORIG_HEAD_PATH,
        REBASE_ONTO_PATH,
        REBASE_PAUSE_PATH,
        REBASE_MSG_PATH,
    ] {
        let _ = fs::remove_file(path);
    }
}

/// Walks strictly along the first-parent chain from `tip` back to `base`
/// (exclusive), collecting the commits to replay in oldest-first order.
/// Bails if a merge commit is encountered before reaching `base`, rather
/// than silently dropping whatever a second parent would have contributed.
pub(crate) fn collect_commits_to_replay(tip: &str, base: &str) -> anyhow::Result<Vec<String>> {
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
                short(&current)
            );
        }

        commits.push(current.clone());

        match parents.first() {
            Some(p) => current = p.clone(),
            None => anyhow::bail!(
                "fatal: reached root commit {} without finding the merge base — histories are unrelated",
                short(&current)
            ),
        }
    }

    commits.reverse();
    Ok(commits)
}