use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use anyhow::Result;

use crate::helpers::apply::{apply_hunks, cached_index_entry, format_apply_numstat, format_apply_stat, format_apply_summary, join_image, load_patch_target, parse_patch, patch_target_exists, read_patch_input, remove_worktree_file, split_image, write_worktree_file};
use crate::helpers::write_object;
use crate::index::{build_entry, read_index, write_index, IndexEntry};
use crate::objects::{ApplyOptions, FilePatch};

/// `rgit apply [--check] [--stat] [--numstat] [--summary] [--apply] [--cached | --index] [-R] [-v] [-p<n>] [<patch>...]`
///
/// Applies a unified or git-style diff (from files, or stdin) to the working tree; `--cached`
/// applies it to the index only and `--index` to both (which must agree beforehand). Creations,
/// deletions, renames, copies, mode changes, `\ No newline at end of file`, and CRLF files are
/// handled. `--stat`, `--numstat`, and `--summary` only describe the patch (add `--apply` to
/// also apply it); `--check` only verifies it. Like git, it is all-or-nothing: every file is
/// checked first, and nothing is written unless every hunk applies. Exits 1 when the patch does
/// not apply and 128 on unreadable or corrupt input.
pub fn apply(opts: ApplyOptions) -> Result<()> {
    match run_apply(&opts) {
        Ok(true) => Ok(()),
        Ok(false) => std::process::exit(1),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(128);
        }
    }
}

fn run_apply(opts: &ApplyOptions) -> Result<bool> {
    let text = read_patch_input(&opts.patches)?;
    let mut patches = parse_patch(&text, opts.strip)?;
    if patches.is_empty() {
        anyhow::bail!("error: No valid patches in input (allow with \"--allow-empty\")");
    }
    if opts.reverse {
        patches = patches.iter().map(FilePatch::reversed).collect();
    }

    if opts.stat {
        print!("{}", format_apply_stat(&patches));
    }
    if opts.numstat {
        print!("{}", format_apply_numstat(&patches));
    }
    if opts.summary {
        print!("{}", format_apply_summary(&patches));
    }

    let do_apply = opts.apply || !(opts.check || opts.stat || opts.numstat || opts.summary);
    if !do_apply && !opts.check {
        return Ok(true);
    }

    let use_index = opts.cached || opts.index;
    let index_entries = read_index()?;
    let index_map: HashMap<String, IndexEntry> = index_entries.iter().map(|e| (e.path.clone(), e.clone())).collect();

    let mut pending: BTreeMap<String, Option<(String, u32)>> = BTreeMap::new();
    let mut failed = false;

    for patch in &patches {
        if opts.verbose {
            eprintln!("Checking patch {}...", verbose_name(patch));
        }
        if !check_patch(patch, opts, &index_map, &mut pending) {
            failed = true;
        }
    }

    if failed {
        return Ok(false);
    }
    if !do_apply {
        return Ok(true);
    }

    if !opts.cached {
        for (path, state) in &pending {
            if state.is_none() && Path::new(path).exists() {
                remove_worktree_file(path)?;
            }
        }
        for (path, state) in &pending {
            if let Some((content, mode)) = state {
                write_worktree_file(path, content, *mode)?;
            }
        }
    }

    if use_index {
        let mut entries: Vec<IndexEntry> = index_entries.into_iter().filter(|e| !pending.contains_key(&e.path)).collect();
        for (path, state) in &pending {
            if let Some((content, mode)) = state {
                let hash_hex = write_object("blob", content.as_bytes())?;
                let mut hash = [0u8; 20];
                hash.copy_from_slice(&hex::decode(&hash_hex)?);
                let mut entry = if opts.cached {
                    cached_index_entry(path, hash, content.len())
                } else {
                    build_entry(path, hash, &fs::metadata(path)?)
                };
                entry.mode = *mode;
                entries.push(entry);
            }
        }
        write_index(&mut entries)?;
    }

    if opts.verbose {
        for patch in &patches {
            eprintln!("Applied patch {} cleanly.", verbose_name(patch));
        }
    }
    Ok(true)
}

fn verbose_name(patch: &FilePatch) -> String {
    match (&patch.old_path, &patch.new_path) {
        (Some(old), Some(new)) if patch.is_rename && old != new => format!("{} => {}", old, new),
        _ => patch.display_name().to_string(),
    }
}

fn check_patch(patch: &FilePatch, opts: &ApplyOptions, index_map: &HashMap<String, IndexEntry>, pending: &mut BTreeMap<String, Option<(String, u32)>>) -> bool {
    let name = patch.display_name().to_string();
    let fail = |messages: &[String]| {
        for m in messages {
            eprintln!("{}", m);
        }
        false
    };

    if patch.is_binary {
        return fail(&[
            format!("error: cannot apply binary patch to '{}' without full index line", name),
            format!("error: {}: patch does not apply", name),
        ]);
    }
    for path in [&patch.old_path, &patch.new_path].into_iter().flatten() {
        if path.is_empty() || Path::new(path).is_absolute() || path.split('/').any(|c| c == ".." || c == ".git") {
            return fail(&[format!("error: invalid path '{}'", path)]);
        }
    }

    let source = if patch.is_new {
        None
    } else {
        let old_path = patch.old_path.as_deref().unwrap_or("");
        match load_patch_target(old_path, opts, index_map, pending) {
            Ok(Some(state)) => Some(state),
            Ok(None) => {
                let reason = if opts.cached || opts.index { "does not exist in index" } else { "No such file or directory" };
                return fail(&[format!("error: {}: {}", old_path, reason)]);
            }
            Err(message) => return fail(&[message]),
        }
    };

    let creates_new_path = patch.is_new || patch.is_rename || patch.is_copy;
    if let (true, Some(new_path)) = (creates_new_path, &patch.new_path) {
        let same_as_source = patch.old_path.as_deref() == Some(new_path.as_str());
        if !same_as_source && patch_target_exists(new_path, opts, index_map, pending) {
            let place = if opts.cached || opts.index { "index" } else { "working directory" };
            return fail(&[format!("error: {}: already exists in {}", new_path, place)]);
        }
    }

    let (old_content, old_mode) = source.unwrap_or_default();
    let image = split_image(&old_content);
    let (result, offsets) = match apply_hunks(image, &patch.hunks) {
        Ok(applied) => applied,
        Err(failure) => {
            let mut messages = Vec::new();
            if opts.verbose {
                messages.push(format!("error: while searching for:\n{}", failure.searched_for));
            }
            messages.push(format!("error: patch failed: {}:{}", name, failure.old_start));
            messages.push(format!("error: {}: patch does not apply", name));
            return fail(&messages);
        }
    };

    if opts.verbose {
        for (hunk, line, offset) in offsets {
            let unit = if offset == 1 { "line" } else { "lines" };
            eprintln!("Hunk #{} succeeded at {} (offset {} {}).", hunk, line, offset, unit);
        }
    }

    let content = join_image(&result);
    if patch.is_delete {
        if !content.is_empty() {
            return fail(&["error: removal patch leaves file contents".to_string(), format!("error: {}: patch does not apply", name)]);
        }
    }

    if patch.is_delete || patch.is_rename {
        if let Some(old_path) = &patch.old_path {
            pending.insert(old_path.clone(), None);
        }
    }
    if !patch.is_delete {
        if let Some(new_path) = &patch.new_path {
            let base_mode = if patch.is_new { 0o100644 } else { old_mode };
            let mode = patch.new_mode.unwrap_or(if base_mode == 0 { 0o100644 } else { base_mode });
            pending.insert(new_path.clone(), Some((content, mode)));
        }
    }
    true
}
