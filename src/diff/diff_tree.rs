pub mod cli;

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use anyhow::Result;

use crate::diff::plumbing::{check_path_argument, diff_trees, parse_diff_plumbing_args, render_diff_changes, reverse_changes, DIFF_COMMON_OPTIONS};
use crate::core::commit::parse_commit_object;
use crate::history::log::engine::{format_log_entry, join_entry_and_diff, parse_log_args};
use crate::workspace::ls_files::engine::normalize_pathspecs;
use crate::core::object_store::read_object;
use crate::core::rev_parse::{peel_to_type, resolve_rev};
use crate::diff::plumbing::types::DiffPlumbingOptions;
use crate::history::log::types::{LogFormat, LogOptions};
use crate::core::repo;

/// The diff of two trees (either may be absent), rendered per the options. Returns the text
/// and whether anything differs.
fn tree_diff_text(old: Option<&str>, new: Option<&str>, opts: &DiffPlumbingOptions, specs: &[String]) -> Result<(String, bool)> {
    // Patch and stat output always look inside directories.
    let mut walk = opts.clone();
    walk.recursive |= opts.patch || opts.stat || opts.shortstat || opts.numstat || opts.summary;
    let mut changes = Vec::new();
    diff_trees(old, new, "", &walk, specs, &mut changes)?;
    if opts.reverse {
        reverse_changes(&mut changes);
    }
    if opts.renames.level > 0 {
        // --find-copies-harder: unchanged files of the starting side are copy sources too.
        let mut start = std::collections::BTreeMap::new();
        if opts.renames.find_copies_harder {
            if let Some(tree) = if opts.reverse { new } else { old } {
                crate::core::object_store::flatten_tree(tree, "", &mut start)?;
            }
            start.retain(|p, _| crate::diff::plumbing::diff_path_selected(p, false, specs));
        }
        let unmodified = crate::diff::plumbing::unmodified_files(&start, &changes);
        changes = crate::diff::rename::detect_renames(changes, &opts.renames, &unmodified);
    }
    Ok((render_diff_changes(&changes, opts)?, !changes.is_empty()))
}

/// Prints one commit the way `diff-tree <commit>` does: against each parent `-m` allows
/// (none for a merge otherwise), a root commit only with `--root`. Returns whether anything
/// differed.
fn show_commit(out: &mut impl Write, hash: &str, parents_override: Option<Vec<String>>, opts: &DiffPlumbingOptions, log_opts: Option<&LogOptions>, specs: &[String], printed: &mut usize) -> Result<bool> {
    let commit = parse_commit_object(hash)?;
    let parents = parents_override.unwrap_or_else(|| commit.parents.clone());
    let against: Vec<Option<String>> = match parents.len() {
        0 if opts.root => vec![None],
        0 => Vec::new(),
        1 => vec![Some(parents[0].clone())],
        _ if opts.merges => parents.iter().cloned().map(Some).collect(),
        _ => Vec::new(),
    };
    let mut differs = false;
    for parent in against {
        let parent_tree = match &parent {
            Some(parent) => Some(parse_commit_object(parent)?.tree),
            None => None,
        };
        let (diff, changed) = tree_diff_text(parent_tree.as_deref(), Some(&commit.tree), opts, specs)?;
        differs |= changed;
        match log_opts {
            Some(log_opts) => {
                let mut entry = format_log_entry(&commit, log_opts, &HashMap::new());
                let multi_line = !matches!(log_opts.format, LogFormat::Oneline | LogFormat::Template { .. });
                if multi_line {
                    // diff-tree never abbreviates: full parents on the `Merge:` line.
                    if commit.parents.len() > 1 {
                        entry = entry.lines().map(|l| if l.starts_with("Merge: ") { format!("Merge: {}\n", commit.parents.join(" ")) } else { format!("{}\n", l) }).collect();
                    }
                    if let Some(parent) = parent.as_ref().filter(|_| parents.len() > 1) {
                        entry = entry.replacen('\n', &format!(" (from {})\n", parent), 1);
                    }
                    if *printed > 0 {
                        out.write_all(b"\n")?;
                    }
                }
                out.write_all(join_entry_and_diff(&entry, &diff, log_opts).as_bytes())?;
                *printed += 1;
            }
            None => {
                // git prints the id whenever something changed, even if the chosen format shows
                // nothing for it (`--summary` of plain modifications).
                if !changed && diff.is_empty() {
                    continue;
                }
                if diff.is_empty() && opts.quiet {
                    continue;
                }
                if !opts.no_commit_id {
                    writeln!(out, "{}", hash)?;
                }
                out.write_all(diff.as_bytes())?;
                *printed += 1;
            }
        }
    }
    Ok(differs)
}

/// `rgit diff-tree [--stdin] [-m] [-s] [-v] [--no-commit-id] [--pretty[=<format>]] [-t]
/// [-r] [--root] [<common diff options>] <tree-ish> [<tree-ish>] [<path>...]`
///
/// Compares the content and mode of two trees. With two tree-ishes, prints what changed
/// from the first to the second. With one commit, prints the commit id and the changes
/// against its parent: nothing for a merge unless `-m` (then once per parent), nothing for
/// a root commit unless `--root`. The default output is raw:
/// `:<old mode> <new mode> <old hash> <new hash> <status> TAB <path>` with status `A`, `D`,
/// `M` or `T`. Without `-r` only the top level is compared, so a changed directory is one
/// entry; `-r` recurses and `-t` also lists the directories. `-p`, `--name-only`,
/// `--name-status`, `--stat`, `--shortstat`, `--numstat`, `--summary`, `-s`, `-z`,
/// `--abbrev[=<n>]` and `-R` choose the format; `--quiet` / `--exit-code` make the exit
/// status 1 when there are differences. `--stdin` reads commits (optionally followed by
/// the parents to compare with) or pairs of trees, one per line. `-v` / `--pretty` /
/// `--format` print the commit header before the diff; `--no-commit-id` drops the id line.
/// Paths limit the comparison. `-M[<n>]` / `-C[<n>]` / `--find-copies-harder` / `-l<n>` detect
/// renames and copies (shown as `R086` / `C100` with both paths). No combined diffs
/// (`-c`, `--cc`). Exit status: 0 (1 with `--exit-code` and differences), 128 on errors,
/// 129 for a usage error.
pub fn diff_tree(args: Vec<String>) -> Result<()> {
    let usage = format!("usage: rgit diff-tree [--stdin] [-m] [-s] [-v] [--no-commit-id] [--pretty]\n              [-t] [-r] [--root] [<common-diff-options>] <tree-ish> [<tree-ish>] [<path>...]\n\n  -r            diff recursively\n  -t            show tree entries as well\n  --root        include the initial commit as diff against /dev/null\n\n{}", DIFF_COMMON_OPTIONS);
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        crate::ui::exit(code);
    };
    let mut opts = match parse_diff_plumbing_args(&args, "diff-tree", &usage) {
        Ok(opts) => opts,
        Err((message, code)) => {
            if code == 129 && message.starts_with("usage:") {
                print!("{}", message);
                crate::ui::exit(code);
            }
            die(&message, code)
        }
    };
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git\n", 128);
    }
    let log_opts = match &opts.pretty {
        Some(pretty) => {
            let pretty = pretty.strip_prefix("format:").map(|f| format!("tformat:{}", f)).unwrap_or_else(|| pretty.clone());
            match parse_log_args(&[format!("--pretty={}", pretty)], false) {
                Ok(mut log_opts) => {
                    // diff-tree never abbreviates commit ids, and the diff flags decide how
                    // the header and the diff are joined.
                    log_opts.revs.abbrev_commit = false;
                    // Plumbing: never colored.
                    log_opts.color = false;
                    log_opts.show_notes = false;
                    log_opts.patch = opts.patch;
                    log_opts.stat = opts.stat;
                    log_opts.shortstat = opts.shortstat;
                    log_opts.name_only = opts.name_only;
                    log_opts.name_status = opts.name_status;
                    Some(log_opts)
                }
                Err(e) => die(&format!("{}\n", e), 128),
            }
        }
        None => None,
    };

    // Leading arguments that name objects are the trees; everything after is a path.
    let mut revs: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    for arg in std::mem::take(&mut opts.args) {
        match resolve_rev(&arg) {
            Ok(hash) if paths.is_empty() && revs.len() < 2 => revs.push(hash),
            _ => {
                if let Err(message) = check_path_argument(&arg) {
                    die(&message, 128);
                }
                paths.push(arg);
            }
        }
    }
    paths.extend(std::mem::take(&mut opts.paths));
    let specs = normalize_pathspecs(&paths);

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut differs = false;
    let mut printed = 0;

    if opts.stdin {
        if !revs.is_empty() {
            die(&usage, 129);
        }
        for line in io::stdin().lock().lines() {
            let line = line?;
            let ids: Vec<&str> = line.split_whitespace().collect();
            let Some(first) = ids.first() else { continue };
            let is_commit = matches!(read_object(first), Ok((ref kind, _)) if kind == "commit");
            if is_commit {
                let parents = (ids.len() > 1).then(|| ids[1..].iter().map(|s| s.to_string()).collect());
                differs |= show_commit(&mut out, first, parents, &opts, log_opts.as_ref(), &specs, &mut printed)?;
            } else if ids.len() == 2 {
                let (Ok(a), Ok(b)) = (peel_to_type(ids[0].to_string(), "tree"), peel_to_type(ids[1].to_string(), "tree")) else {
                    out.flush()?;
                    die(&format!("error: Object {} not found\n", line), 128)
                };
                let (diff, changed) = tree_diff_text(Some(&a), Some(&b), &opts, &specs)?;
                differs |= changed;
                if !opts.quiet {
                    writeln!(out, "{} {}", ids[0], ids[1])?;
                }
                out.write_all(diff.as_bytes())?;
            } else {
                out.flush()?;
                die(&format!("error: Object {} not found\n", first), 128);
            }
            out.flush()?;
        }
    } else {
        match revs.as_slice() {
            [single] => {
                let commit = match peel_to_type(single.clone(), "commit") {
                    Ok(commit) => commit,
                    Err(_) => {
                        let kind = read_object(single).map(|(kind, _)| kind).unwrap_or_else(|_| "unknown object".to_string());
                        eprintln!("error: object {} is a {}, not a commit", single, kind);
                        return Ok(());
                    }
                };
                differs = show_commit(&mut out, &commit, None, &opts, log_opts.as_ref(), &specs, &mut printed)?;
            }
            [a, b] => {
                let tree = |hash: &String| -> String {
                    match peel_to_type(hash.clone(), "tree") {
                        Ok(tree) => tree,
                        Err(_) => die(&format!("fatal: unable to read tree {}\n", hash), 128),
                    }
                };
                let (diff, changed) = tree_diff_text(Some(&tree(a)), Some(&tree(b)), &opts, &specs)?;
                differs = changed;
                out.write_all(diff.as_bytes())?;
            }
            _ => die(&usage, 129),
        }
    }
    out.flush()?;
    crate::diff::rename::warn_rename_limit("diff.renameLimit");
    if (opts.exit_code || opts.quiet) && differs {
        crate::ui::exit(1);
    }
    Ok(())
}
