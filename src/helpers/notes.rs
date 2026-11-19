use std::collections::BTreeMap;
use std::fs;
use crate::core::commit::{current_signature, is_reachable, write_commit_object};
use crate::core::config::get_config_value;
use crate::helpers::format_patch::{parse_commit_object, split_commit_message};
use crate::helpers::merge::find_merge_base;
use crate::core::object_store::{flatten_tree, read_object, write_object};
use crate::helpers::rebase::run_editor;
use crate::core::rev_parse::resolve_rev;
use crate::helpers::state_files::build_tree_from_map;
use crate::objects::{NotesMergeStrategy, NotesTree};
use crate::core::refs::{read_ref, write_ref};

/// Usage text for `notes`, printed for `-h` and after usage errors (exit 129).
pub const NOTES_USAGE: &str = "usage: rgit notes [--ref <notes-ref>] [list [<object>]]
   or: rgit notes [--ref <notes-ref>] add [-f] [--allow-empty] [-F <file> | -m <msg> | (-c | -C) <object>] [<object>]
   or: rgit notes [--ref <notes-ref>] copy [-f] <from-object> [<to-object>]
   or: rgit notes [--ref <notes-ref>] append [--allow-empty] [-F <file> | -m <msg> | (-c | -C) <object>] [<object>]
   or: rgit notes [--ref <notes-ref>] edit [--allow-empty] [<object>]
   or: rgit notes [--ref <notes-ref>] show [<object>]
   or: rgit notes [--ref <notes-ref>] merge [-s <strategy>] <notes-ref>
   or: rgit notes [--ref <notes-ref>] remove [--ignore-missing] [<object>...]
   or: rgit notes [--ref <notes-ref>] prune [-n] [-v]
   or: rgit notes [--ref <notes-ref>] get-ref
";

/// git's `expand_notes_ref`: `foo` means `refs/notes/foo` and `notes/foo` means `refs/notes/foo`.
// Used by `notes --ref` and `notes merge`.
pub fn expand_notes_ref(name: &str) -> String {
    if name.starts_with("refs/notes/") {
        name.to_string()
    } else if name.starts_with("notes/") {
        format!("refs/{}", name)
    } else {
        format!("refs/notes/{}", name)
    }
}

/// The notes ref in effect when `--ref` isn't given, like git's `default_notes_ref`:
/// `GIT_NOTES_REF`, then `core.notesRef`, then `refs/notes/commits`.
// Used by `notes` and by `log` / `show` when displaying notes.
pub fn default_notes_ref() -> String {
    std::env::var("GIT_NOTES_REF")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| get_config_value("core.notesref").ok().flatten().filter(|v| !v.is_empty()))
        .unwrap_or_else(|| "refs/notes/commits".to_string())
}

/// Loads the notes tree a notes ref points at: every blob whose path, with the fan-out
/// slashes removed, is a 40-hex object name is that object's note; anything else in the
/// tree is kept aside so rewriting the tree doesn't lose it. A missing ref is an empty tree.
// Used by `notes` (every subcommand) and by `log` / `show` when displaying notes.
pub fn load_notes_tree(notes_ref: &str) -> anyhow::Result<NotesTree> {
    let mut tree = NotesTree { notes_ref: notes_ref.to_string(), ..Default::default() };
    let Some(commit) = read_ref(notes_ref).ok().flatten() else { return Ok(tree) };
    load_notes_commit(&commit, &mut tree)?;
    tree.commit = Some(commit);
    Ok(tree)
}

/// Fills `tree` with the notes (and other entries) of the notes commit `commit`.
// Used by `load_notes_tree` and `merge_notes`.
fn load_notes_commit(commit: &str, tree: &mut NotesTree) -> anyhow::Result<()> {
    let mut flat = BTreeMap::new();
    flatten_tree(&parse_commit_object(commit)?.tree, "", &mut flat)?;
    for (path, (hash, mode)) in flat {
        let object: String = path.chars().filter(|c| *c != '/').collect();
        let fanned_out = path.split('/').rev().skip(1).all(|part| part.len() == 2);
        if object.len() == 40 && object.bytes().all(|b| b.is_ascii_hexdigit()) && fanned_out {
            tree.notes.insert(object.to_ascii_lowercase(), hex::encode(hash));
        } else {
            tree.other.insert(path, (hash, mode));
        }
    }
    Ok(())
}

/// Assigns each annotated object its path in the notes tree with git's fan-out rule
/// (`determine_fanout`): notes stay flat until, at an even depth, all sixteen next hex
/// digits lead to more than one note; then that level becomes a two-character directory.
// Used by `commit_notes_tree`.
fn note_paths(objects: &[&String], depth: usize, mut fanout: usize, out: &mut Vec<(String, String)>) {
    let groups: Vec<Vec<&String>> = (0..16u32).map(|digit| objects.iter().copied().filter(|o| o[depth..=depth].eq_ignore_ascii_case(&format!("{:x}", digit))).collect()).collect();
    if depth % 2 == 0 && depth <= 2 * fanout && groups.iter().all(|group| group.len() > 1) {
        fanout += 1;
    }
    for group in groups {
        match group.as_slice() {
            [] => {}
            [object] => {
                let mut path = String::new();
                for level in 0..fanout {
                    path.push_str(&object[level * 2..level * 2 + 2]);
                    path.push('/');
                }
                path.push_str(&object[fanout * 2..]);
                out.push(((*object).clone(), path));
            }
            _ => note_paths(&group, depth + 1, fanout, out),
        }
    }
}

/// Port of git's `commit_notes`: writes the notes as a tree (plus the non-note entries it
/// had), commits it on top of the ref's current commit (or `parents`, for a merge) with
/// `message` (used as given: git ends ordinary notes commits with a newline but not notes
/// merges), and moves the notes ref to the new commit.
// Used by `notes add`, `append`, `edit`, `copy`, `remove`, `prune` and `merge`.
pub fn commit_notes_tree(tree: &NotesTree, message: &str, parents: Option<Vec<String>>) -> anyhow::Result<String> {
    let mut map: BTreeMap<String, ([u8; 20], u32)> = tree.other.clone();
    let objects: Vec<&String> = tree.notes.keys().collect();
    let mut paths = Vec::new();
    if !objects.is_empty() {
        note_paths(&objects, 0, 0, &mut paths);
    }
    for (object, path) in paths {
        let mut hash = [0u8; 20];
        hex::decode_to_slice(&tree.notes[&object], &mut hash)?;
        map.insert(path, (hash, 0o100644));
    }
    let tree_hash = if map.is_empty() { write_object("tree", b"")? } else { build_tree_from_map(&map)? };
    let parents = parents.unwrap_or_else(|| tree.commit.iter().cloned().collect());
    let signature = current_signature()?;
    let commit = write_commit_object(&tree_hash, &parents, &signature, &signature, message)?;
    write_ref(&tree.notes_ref, &commit)?;
    Ok(commit)
}

/// git's `strbuf_stripspace`: drops trailing whitespace on every line, leading and trailing
/// blank lines and repeated blank lines, removes `#` comment lines when `strip_comments`,
/// and ends non-empty text with a newline.
// Used by `notes add`, `append` and `edit` to clean up note text.
pub fn stripspace(text: &str, strip_comments: bool) -> String {
    let mut out = String::new();
    let mut pending_blank = false;
    for line in text.lines() {
        if strip_comments && line.starts_with('#') {
            continue;
        }
        let line = line.trim_end();
        if line.is_empty() {
            pending_blank = !out.is_empty();
            continue;
        }
        if pending_blank {
            out.push('\n');
            pending_blank = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Opens the editor on `.git/NOTES_EDITMSG`, pre-filled with `initial` and a commented
/// reminder of which object the note is for, and returns the cleaned-up result.
// Used by `notes add`, `append` and `edit` when no message is given (or with `-c` / `-e`).
pub fn edit_note_text(initial: &str, object: &str) -> anyhow::Result<String> {
    let mut template = initial.to_string();
    if !template.is_empty() && !template.ends_with('\n') {
        template.push('\n');
    }
    template.push_str("\n#\n# Write/edit the notes for the following object:\n#\n");
    template.push_str(&format!("# {}\n", describe_object(object)));
    template.push_str("#\n");
    let path = crate::core::repo::path(".git/NOTES_EDITMSG");
    fs::write(&path, template)?;
    run_editor(&path, false).map_err(|_| anyhow::anyhow!("fatal: please supply the note contents using either -m or -F option"))?;
    Ok(stripspace(&fs::read_to_string(&path)?, true))
}

/// One line naming an object for the editor template: `commit <hash> <subject>` for
/// commits, `<type> <hash>` otherwise.
// Used by `edit_note_text`.
fn describe_object(object: &str) -> String {
    match parse_commit_object(object) {
        Ok(commit) => format!("commit {} {}", object, split_commit_message(&commit.message).0),
        Err(_) => format!("{} {}", read_object(object).map(|(kind, _)| kind).unwrap_or_else(|_| "object".to_string()), object),
    }
}

/// Reads a note blob as text; `None` when the blob can't be read.
// Used by `notes show`, `append`, `edit`, `merge`, and by `log` / `show` when displaying notes.
pub fn read_note_text(blob: &str) -> Option<String> {
    read_object(blob).ok().map(|(_, content)| String::from_utf8_lossy(&content).into_owned())
}

/// Returns the note attached to `object` in the default notes ref, for display under a
/// commit; the notes tree is loaded once per process.
// Used by `log` and `show` (the `Notes:` block and the `%N` placeholder).
pub fn display_note(object: &str) -> Option<String> {
    static NOTES: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    let notes = NOTES.get_or_init(|| load_notes_tree(&default_notes_ref()).map(|tree| tree.notes).unwrap_or_default());
    notes.get(object).and_then(|blob| read_note_text(blob))
}

/// git's `combine_notes_concatenate`: the two notes separated by a blank line.
// Used by `merge_notes` (`-s union`).
fn concatenate_notes(current: &str, new: &str) -> String {
    if new.is_empty() {
        return current.to_string();
    }
    if current.is_empty() {
        return new.to_string();
    }
    format!("{}\n\n{}", current.strip_suffix('\n').unwrap_or(current), new)
}

/// git's `combine_notes_cat_sort_uniq`: the non-empty lines of both notes, sorted, without duplicates.
// Used by `merge_notes` (`-s cat_sort_uniq`).
fn cat_sort_uniq_notes(current: &str, new: &str) -> String {
    let lines: std::collections::BTreeSet<&str> = current.lines().chain(new.lines()).filter(|line| !line.is_empty()).collect();
    lines.into_iter().map(|line| format!("{}\n", line)).collect()
}

/// Port of the automatic part of git's `notes_merge`: merges the notes ref `remote_ref`
/// into `local`. Returns the messages to print. When one side contains the other the ref
/// is fast-forwarded (or already up to date); otherwise the notes are merged three ways
/// against the merge base, and a note changed differently on both sides is resolved by
/// `strategy` (`ours`, `theirs`, `union`, `cat_sort_uniq`). The default `manual` strategy
/// reports the conflicts and fails, because rgit has no `NOTES_MERGE_WORKTREE` workflow.
// Used by `notes merge`.
pub fn merge_notes(local: &mut NotesTree, remote_ref: &str, strategy: NotesMergeStrategy) -> anyhow::Result<Vec<String>> {
    let Some(remote_commit) = read_ref(remote_ref).ok().flatten().or_else(|| resolve_rev(remote_ref).ok()) else {
        anyhow::bail!("fatal: failed to resolve '{}' as a valid ref.", remote_ref);
    };
    let Some(local_commit) = local.commit.clone() else {
        write_ref(&local.notes_ref, &remote_commit)?;
        return Ok(Vec::new());
    };
    if local_commit == remote_commit || is_reachable(&local_commit, &remote_commit)? {
        return Ok(vec!["Already up to date.".to_string()]);
    }
    if is_reachable(&remote_commit, &local_commit)? {
        write_ref(&local.notes_ref, &remote_commit)?;
        return Ok(vec!["Fast-forward".to_string()]);
    }

    let (mut base, mut remote) = (NotesTree::default(), NotesTree::default());
    if let Some(base_commit) = find_merge_base(&local_commit, &remote_commit)? {
        load_notes_commit(&base_commit, &mut base)?;
    }
    load_notes_commit(&remote_commit, &mut remote)?;

    let mut messages = Vec::new();
    let mut conflicts = Vec::new();
    let objects: std::collections::BTreeSet<&String> = base.notes.keys().chain(remote.notes.keys()).collect();
    for object in objects {
        let (b, l, r) = (base.notes.get(object), local.notes.get(object).cloned(), remote.notes.get(object));
        if r == b || r == l.as_ref() {
            continue;
        }
        if l.as_ref() == b {
            match r {
                Some(note) => local.notes.insert(object.clone(), note.clone()),
                None => local.notes.remove(object),
            };
            continue;
        }
        let text = |note: Option<&String>| note.and_then(|blob| read_note_text(blob)).unwrap_or_default();
        let kind = match (b, &l, r) {
            (None, _, _) => "add/add",
            (_, None, _) | (_, _, None) => "delete/modify",
            _ => "content",
        };
        match strategy {
            NotesMergeStrategy::Manual => {
                messages.push(format!("Auto-merging notes for {}", object));
                messages.push(format!("CONFLICT ({}): Merge conflict in notes for object {}", kind, object));
                conflicts.push(object.clone());
            }
            NotesMergeStrategy::Ours => messages.push(format!("Using local notes for {}", object)),
            NotesMergeStrategy::Theirs => {
                messages.push(format!("Using remote notes for {}", object));
                match r {
                    Some(note) => local.notes.insert(object.clone(), note.clone()),
                    None => local.notes.remove(object),
                };
            }
            NotesMergeStrategy::Union | NotesMergeStrategy::CatSortUniq => {
                let combined = if strategy == NotesMergeStrategy::Union {
                    messages.push(format!("Concatenating local and remote notes for {}", object));
                    concatenate_notes(&text(l.as_ref()), &text(r))
                } else {
                    messages.push(format!("Concatenating unique lines in local and remote notes for {}", object));
                    cat_sort_uniq_notes(&text(l.as_ref()), &text(r))
                };
                local.notes.insert(object.clone(), write_object("blob", combined.as_bytes())?);
            }
        }
    }

    if !conflicts.is_empty() {
        for message in &messages {
            println!("{}", message);
        }
        anyhow::bail!("Automatic notes merge failed. rgit cannot resolve notes conflicts by hand; rerun with -s ours, theirs, union or cat_sort_uniq.");
    }
    commit_notes_tree(local, &format!("Merged notes from {} into {}", remote_ref, local.notes_ref), Some(vec![local_commit, remote_commit]))?;
    Ok(messages)
}
