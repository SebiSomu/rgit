use std::collections::HashMap;
use anyhow::Result;

use crate::helpers::apply::{check_file_patch, format_apply_numstat, format_apply_stat, format_apply_summary, parse_patch, read_patch_input, write_planned_changes};
use crate::index::{read_index, IndexEntry};
use crate::objects::{ApplyOptions, FilePatch, PlannedChanges};

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

    let mut pending = PlannedChanges::new();
    let mut failed = false;

    for patch in &patches {
        if opts.verbose {
            eprintln!("Checking patch {}...", patch.verbose_name());
        }
        if !check_file_patch(patch, opts, &index_map, &mut pending) {
            failed = true;
        }
    }

    if failed {
        return Ok(false);
    }
    if !do_apply {
        return Ok(true);
    }

    write_planned_changes(&pending, opts.cached, use_index, index_entries)?;

    if opts.verbose {
        for patch in &patches {
            eprintln!("Applied patch {} cleanly.", patch.verbose_name());
        }
    }
    Ok(true)
}
