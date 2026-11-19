use std::fs;
use anyhow::Result;

use crate::helpers::notes::{commit_notes_tree, default_notes_ref, edit_note_text, expand_notes_ref, load_notes_tree, merge_notes, read_note_text, stripspace, NOTES_USAGE};
use crate::core::object_store::{read_object, write_object};
use crate::core::rev_parse::resolve_rev;
use crate::objects::{NotesMergeStrategy, NotesTree};
use crate::core::repo;

/// `rgit notes [--ref <notes-ref>] <subcommand>`
///
/// Attaches notes to objects (usually commits) without changing them. Notes live in a
/// separate history under `refs/notes/commits` (or `--ref`, `GIT_NOTES_REF`,
/// `core.notesRef`): a tree with one blob per annotated object, laid out like git's so both
/// tools read each other's notes. `log` and `show` print a commit's note in a `Notes:` block
/// (and as `%N`). Subcommands: `list [<object>]` (the default) prints `<note> <object>`
/// pairs; `add [-f] [--allow-empty] [-m <msg> | -F <file> | -C <object> | -c <object>]
/// [<object>]` attaches a note (several `-m` become paragraphs; without a message the
/// editor opens; `-f` overwrites); `append` adds a paragraph to an existing note; `edit`
/// opens it in the editor; `copy [-f] <from> [<to>]` copies a note; `show [<object>]` prints
/// it; `remove [--ignore-missing] [<object>...]` deletes notes; `prune [-n] [-v]` drops
/// notes of objects that no longer exist; `merge [-s <strategy>] <notes-ref>` merges another
/// notes ref (`ours`, `theirs`, `union`, `cat_sort_uniq`; conflicts under the default
/// `manual` strategy are reported but can't be resolved by hand in rgit); and `get-ref`
/// prints the notes ref in use. `<object>` defaults to `HEAD`. An empty note removes the
/// note unless `--allow-empty`. Exit status, like git: 0 success, 1 failure, 129 usage.
pub fn notes(args: Vec<String>) -> Result<()> {
    if args.iter().any(|a| a == "-h") {
        print!("{}", NOTES_USAGE);
        crate::ui::exit(129);
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }

    let mut notes_ref = default_notes_ref();
    let mut rest: &[String] = &args;
    loop {
        match rest {
            [flag, value, tail @ ..] if flag == "--ref" => {
                notes_ref = expand_notes_ref(value);
                rest = tail;
            }
            [flag, tail @ ..] if flag.starts_with("--ref=") => {
                notes_ref = expand_notes_ref(&flag["--ref=".len()..]);
                rest = tail;
            }
            _ => break,
        }
    }
    let (subcommand, sub_args) = match rest.split_first() {
        Some((name, tail)) if !name.starts_with('-') => (name.as_str(), tail),
        _ => ("list", rest),
    };

    let result = match subcommand {
        "list" => notes_list(&notes_ref, sub_args),
        "add" | "append" | "edit" => notes_write(&notes_ref, subcommand, sub_args),
        "copy" => notes_copy(&notes_ref, sub_args),
        "show" => notes_show(&notes_ref, sub_args),
        "remove" => notes_remove(&notes_ref, sub_args),
        "prune" => notes_prune(&notes_ref, sub_args),
        "merge" => notes_merge(&notes_ref, sub_args),
        "get-ref" if sub_args.is_empty() => {
            println!("{}", notes_ref);
            Ok(())
        }
        "get-ref" => usage_error("error: too many arguments"),
        other => usage_error(&format!("error: unknown subcommand: `{}'", other)),
    };
    if let Err(e) = result {
        eprintln!("{}", e);
        crate::ui::exit(if e.to_string().starts_with("fatal:") { 128 } else { 1 });
    }
    Ok(())
}

fn usage_error(message: &str) -> ! {
    eprintln!("{}", message);
    eprint!("{}", NOTES_USAGE);
    crate::ui::exit(129);
}

/// Resolves the object a subcommand works on (`HEAD` when none is named).
fn target_object(name: Option<&String>) -> Result<String> {
    let name = name.map(String::as_str).unwrap_or("HEAD");
    resolve_rev(name).map_err(|_| anyhow::anyhow!("fatal: failed to resolve '{}' as a valid ref.", name))
}

fn notes_list(notes_ref: &str, args: &[String]) -> Result<()> {
    if args.len() > 1 {
        usage_error("error: too many arguments");
    }
    let tree = load_notes_tree(notes_ref)?;
    match args.first() {
        None => {
            for (object, note) in &tree.notes {
                println!("{} {}", note, object);
            }
        }
        Some(name) => {
            let object = target_object(Some(name))?;
            match tree.notes.get(&object) {
                Some(note) => println!("{}", note),
                None => anyhow::bail!("error: no note found for object {}.", object),
            }
        }
    }
    Ok(())
}

/// `add`, `append` and `edit`: build the note text from `-m` / `-F` / `-C` / `-c` or the
/// editor, then store it (or remove the note when the text is empty).
fn notes_write(notes_ref: &str, subcommand: &str, args: &[String]) -> Result<()> {
    let (mut force, mut allow_empty, mut use_editor, mut given) = (false, false, false, false);
    let mut pieces: Vec<String> = Vec::new();
    let mut positional: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        let mut value = |glued: &str, name: &str| -> String {
            if !glued.is_empty() {
                return glued.to_string();
            }
            let v = args.get(i).cloned().unwrap_or_else(|| usage_error(&format!("error: switch `{}' requires a value", name)));
            i += 1;
            v
        };
        match arg {
            "-f" | "--force" if subcommand == "add" => force = true,
            "--allow-empty" => allow_empty = true,
            "-e" | "--edit" => use_editor = true,
            _ if arg.starts_with("-m") || arg == "--message" || arg.starts_with("--message=") => {
                let glued = arg.strip_prefix("--message=").or_else(|| if arg == "--message" { Some("") } else { arg.strip_prefix("-m") }).unwrap_or("");
                pieces.push(stripspace(&value(glued, "m"), false));
                given = true;
            }
            _ if arg.starts_with("-F") || arg == "--file" || arg.starts_with("--file=") => {
                let glued = arg.strip_prefix("--file=").or_else(|| if arg == "--file" { Some("") } else { arg.strip_prefix("-F") }).unwrap_or("");
                let path = value(glued, "F");
                let text = fs::read_to_string(&path).map_err(|_| anyhow::anyhow!("fatal: could not open or read '{}': No such file or directory", path))?;
                pieces.push(stripspace(&text, false));
                given = true;
            }
            _ if arg.starts_with("-C") || arg.starts_with("-c") => {
                let name = value(&arg[2..], &arg[1..2]);
                let blob = resolve_rev(&name).map_err(|_| anyhow::anyhow!("fatal: failed to resolve '{}' as a valid ref.", name))?;
                match read_object(&blob) {
                    Ok((kind, content)) if kind == "blob" => pieces.push(String::from_utf8_lossy(&content).into_owned()),
                    _ => anyhow::bail!("fatal: cannot read note data from non-blob object '{}'.", name),
                }
                use_editor = use_editor || arg.starts_with("-c");
                given = true;
            }
            "--" => positional.extend(args[i..].iter()),
            _ if arg.starts_with('-') => usage_error(&format!("error: unknown option `{}'", arg.trim_start_matches('-'))),
            _ => positional.push(&args[i - 1]),
        }
        if arg == "--" {
            break;
        }
    }
    // Like git, an empty message counts as none given and opens the editor.
    given = given && pieces.iter().any(|piece| !piece.is_empty());
    if positional.len() > 1 {
        usage_error("error: too many arguments");
    }
    let object = target_object(positional.first().copied())?;
    let mut tree = load_notes_tree(notes_ref)?;
    let existing = tree.notes.get(&object).cloned();

    let mut subcommand = subcommand;
    if subcommand == "add" {
        if let Some(_note) = &existing {
            if !force {
                if given {
                    anyhow::bail!("error: Cannot add notes. Found existing notes for object {}. Use '-f' to overwrite existing notes", object);
                }
                subcommand = "edit";
            } else {
                eprintln!("Overwriting existing notes for object {}", object);
            }
        }
    }

    // Several messages become paragraphs separated by a blank line.
    let mut text = pieces.iter().filter(|piece| !piece.is_empty()).cloned().collect::<Vec<_>>().join("\n");
    let previous = existing.as_ref().and_then(|note| read_note_text(note)).unwrap_or_default();
    if !given || use_editor {
        let initial = match subcommand {
            "edit" if !given => previous.clone(),
            _ => text.clone(),
        };
        text = edit_note_text(&initial, &object)?;
    }
    if subcommand == "append" && !previous.is_empty() {
        text = if text.is_empty() { previous } else { format!("{}\n{}", previous, text) };
    }

    if !text.is_empty() || allow_empty {
        tree.notes.insert(object, write_object("blob", text.as_bytes())?);
        commit_notes_tree(&tree, &format!("Notes added by 'git notes {}'\n", subcommand), None)?;
    } else {
        eprintln!("Removing note for object {}", object);
        if tree.notes.remove(&object).is_some() || tree.commit.is_some() {
            commit_notes_tree(&tree, &format!("Notes removed by 'git notes {}'\n", subcommand), None)?;
        }
    }
    Ok(())
}

fn notes_copy(notes_ref: &str, args: &[String]) -> Result<()> {
    let force = args.iter().any(|a| a == "-f" || a == "--force");
    let names: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if names.is_empty() {
        usage_error("error: too few arguments");
    }
    if names.len() > 2 {
        usage_error("error: too many arguments");
    }
    let from = target_object(Some(names[0]))?;
    let to = target_object(names.get(1).copied())?;
    let mut tree = load_notes_tree(notes_ref)?;
    if tree.notes.contains_key(&to) {
        if !force {
            anyhow::bail!("error: Cannot copy notes. Found existing notes for object {}. Use '-f' to overwrite existing notes", to);
        }
        eprintln!("Overwriting existing notes for object {}", to);
    }
    let Some(note) = tree.notes.get(&from).cloned() else {
        anyhow::bail!("error: missing notes on source object {}. Cannot copy.", from);
    };
    tree.notes.insert(to, note);
    commit_notes_tree(&tree, "Notes added by 'git notes copy'\n", None)?;
    Ok(())
}

fn notes_show(notes_ref: &str, args: &[String]) -> Result<()> {
    if args.len() > 1 {
        usage_error("error: too many arguments");
    }
    let object = target_object(args.first())?;
    let tree = load_notes_tree(notes_ref)?;
    match tree.notes.get(&object).and_then(|note| read_note_text(note)) {
        Some(text) => print!("{}", text),
        None => anyhow::bail!("error: no note found for object {}.", object),
    }
    Ok(())
}

fn notes_remove(notes_ref: &str, args: &[String]) -> Result<()> {
    let ignore_missing = args.iter().any(|a| a == "--ignore-missing");
    let mut names: Vec<String> = args.iter().filter(|a| !a.starts_with("--")).cloned().collect();
    if names.is_empty() {
        names.push("HEAD".to_string());
    }
    let mut tree = load_notes_tree(notes_ref)?;
    let mut failed = false;
    for name in &names {
        let Ok(object) = resolve_rev(name) else {
            eprintln!("error: Failed to resolve '{}' as a valid ref.", name);
            failed = true;
            continue;
        };
        if tree.notes.remove(&object).is_some() {
            eprintln!("Removing note for object {}", name);
        } else {
            eprintln!("Object {} has no note", name);
            failed = failed || !ignore_missing;
        }
    }
    if failed {
        crate::ui::exit(1);
    }
    commit_notes_tree(&tree, "Notes removed by 'git notes remove'\n", None)?;
    Ok(())
}

fn notes_prune(notes_ref: &str, args: &[String]) -> Result<()> {
    let dry_run = args.iter().any(|a| a == "-n" || a == "--dry-run");
    let verbose = args.iter().any(|a| a == "-v" || a == "--verbose");
    if args.iter().any(|a| !matches!(a.as_str(), "-n" | "--dry-run" | "-v" | "--verbose")) {
        usage_error("error: too many arguments");
    }
    let mut tree: NotesTree = load_notes_tree(notes_ref)?;
    let gone: Vec<String> = tree.notes.keys().filter(|object| read_object(object).is_err()).cloned().collect();
    for object in &gone {
        if verbose || dry_run {
            println!("{}", object);
        }
        tree.notes.remove(object);
    }
    if !dry_run && tree.commit.is_some() {
        commit_notes_tree(&tree, "Notes removed by 'git notes prune'\n", None)?;
    }
    Ok(())
}

fn notes_merge(notes_ref: &str, args: &[String]) -> Result<()> {
    let mut strategy = NotesMergeStrategy::Manual;
    let mut remote: Option<&String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        let name = if arg == "-s" || arg == "--strategy" {
            i += 1;
            args.get(i - 1).cloned().unwrap_or_else(|| usage_error("error: switch `s' requires a value"))
        } else if let Some(value) = arg.strip_prefix("--strategy=").or_else(|| arg.strip_prefix("-s").filter(|v| !v.is_empty())) {
            value.to_string()
        } else if matches!(arg, "-q" | "--quiet" | "-v" | "--verbose") {
            continue;
        } else if matches!(arg, "--commit" | "--abort") {
            anyhow::bail!("error: rgit keeps no notes merge in progress; there is nothing to {}", &arg[2..]);
        } else if arg.starts_with('-') {
            usage_error(&format!("error: unknown option `{}'", arg.trim_start_matches('-')))
        } else {
            remote = Some(&args[i - 1]);
            continue;
        };
        strategy = match name.as_str() {
            "manual" => NotesMergeStrategy::Manual,
            "ours" => NotesMergeStrategy::Ours,
            "theirs" => NotesMergeStrategy::Theirs,
            "union" => NotesMergeStrategy::Union,
            "cat_sort_uniq" => NotesMergeStrategy::CatSortUniq,
            other => usage_error(&format!("error: unknown -s/--strategy: {}", other)),
        };
    }
    let Some(remote) = remote else { usage_error("error: must specify a notes ref to merge") };
    let mut tree = load_notes_tree(notes_ref)?;
    for message in merge_notes(&mut tree, &expand_notes_ref(remote), strategy)? {
        println!("{}", message);
    }
    Ok(())
}
