use std::collections::BTreeMap;
use std::io::Write;

use crate::helpers::diff_plumbing::{
    diff_files_changes, diff_index_changes, diff_trees, parse_diff_plumbing_args, render_diff_changes, reverse_changes, unmodified_files, DIFF_COMMON_OPTIONS,
};
use crate::helpers::ls_files::normalize_pathspecs;
use crate::helpers::merge::find_merge_base;
use crate::helpers::objects::flatten_tree;
use crate::helpers::rename::{detect_renames, warn_rename_limit};
use crate::helpers::rev_parse::{peel_to_type, resolve_rev};
use crate::index::read_index;
use crate::objects::{DiffChange, DiffSide};
use crate::refs;

type TreeMap = BTreeMap<String, ([u8; 20], u32)>;

/// `rgit diff [<options>] [--cached|--staged] [<commit> [<commit>]] [--] [<path>...]`
///
/// git's porcelain diff, built on the raw diff engine of `diff-files` / `diff-index` /
/// `diff-tree`: the working tree against the index (no revision), the index against `HEAD` or
/// a commit (`--cached` / `--staged`), the working tree against a commit (one revision), or two
/// commits (`A B`, `A..B`, or `A...B` from their merge base). The patch is shown unless another
/// format is asked for (`--stat`, `--shortstat`, `--numstat`, `--summary`, `--name-only`,
/// `--name-status`, `--raw`, `-s`); `-z`, `-R`, `--abbrev`, `--exit-code` and `--quiet` work as
/// for the raw commands. Renames are detected by default (`diff.renames`; `-M[<n>]`, `-C[<n>]`,
/// `--find-copies-harder`, `-l<n>`, `--no-renames`). Unmerged paths show as `U` (`* Unmerged
/// path`); with no revision, a path with both sides gets a combined diff (`diff --cc`) unless
/// a stage is chosen with `-1`/`-2`/`-3` (`--base`/`--ours`/`--theirs`). Files left out by a sparse checkout count
/// as unchanged. Exit status: 0 (1 with `--exit-code` and differences), 128 on errors.
pub fn diff(args: Vec<String>) -> anyhow::Result<()> {
    let usage = format!("usage: rgit diff [<options>] [<commit>] [--] [<path>...]\n   or: rgit diff [<options>] --cached [<commit>] [--] [<path>...]\n   or: rgit diff [<options>] <commit> [<commit>...] <commit> [--] [<path>...]\n\n{}", DIFF_COMMON_OPTIONS);
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        std::process::exit(code);
    };
    let mut opts = match parse_diff_plumbing_args(&args, "diff", &usage) {
        Ok(opts) => opts,
        Err((message, code)) => die(&message, code),
    };
    let explicit = opts.raw || opts.name_only || opts.name_status || opts.stat || opts.shortstat || opts.numstat || opts.summary || opts.no_output;
    if !explicit {
        opts.patch = true;
    }
    opts.recursive = true;

    // Revisions first, then paths; without `--`, an argument that is no revision must be a path.
    let mut revs: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    for arg in std::mem::take(&mut opts.args) {
        if paths.is_empty() && (resolve_rev(&arg).is_ok() || arg.contains("..")) {
            revs.push(arg);
        } else if std::path::Path::new(&arg).exists() {
            paths.push(arg);
        } else {
            die(&format!("fatal: ambiguous argument '{}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'\n", arg), 128);
        }
    }
    paths.extend(std::mem::take(&mut opts.paths));
    let specs = normalize_pathspecs(&paths);

    let tree_of = |rev: &str| -> TreeMap {
        let Ok(hash) = resolve_rev(rev) else { die(&format!("fatal: bad revision '{}'\n", rev), 128) };
        let Ok(tree) = peel_to_type(hash, "tree") else { die(&format!("fatal: bad revision '{}'\n", rev), 128) };
        let mut map = TreeMap::new();
        let _ = flatten_tree(&tree, "", &mut map);
        map
    };
    let tree_hash_of = |rev: &str| -> String {
        let Ok(hash) = resolve_rev(rev) else { die(&format!("fatal: bad revision '{}'\n", rev), 128) };
        peel_to_type(hash, "tree").unwrap_or_else(|_| die(&format!("fatal: bad revision '{}'\n", rev), 128))
    };
    // `A..B` and `A...B` (an empty side means HEAD).
    if revs.len() == 1 && revs[0].contains("..") {
        let range = revs.remove(0);
        let (a, b, symmetric) = match range.split_once("...") {
            Some((a, b)) => (a.to_string(), b.to_string(), true),
            None => {
                let (a, b) = range.split_once("..").unwrap_or((&range, ""));
                (a.to_string(), b.to_string(), false)
            }
        };
        let (a, b) = (if a.is_empty() { "HEAD".to_string() } else { a }, if b.is_empty() { "HEAD".to_string() } else { b });
        if symmetric {
            let (Ok(ca), Ok(cb)) = (resolve_rev(&a), resolve_rev(&b)) else { die(&format!("fatal: bad revision '{}'\n", range), 128) };
            match find_merge_base(&ca, &cb)? {
                Some(base) => revs = vec![base, b],
                None => die(&format!("fatal: {}...{}: no merge base\n", a, b), 128),
            }
        } else {
            revs = vec![a, b];
        }
    }

    let index = read_index().unwrap_or_default();
    let skipped: std::collections::HashSet<String> = index.iter().filter(|e| e.skip_worktree).map(|e| e.path.clone()).collect();
    let mut combined_text = String::new();
    let (mut changes, start): (Vec<DiffChange>, TreeMap) = match (opts.cached, revs.len()) {
        (false, 0) => {
            let tracked: Vec<_> = index.iter().filter(|e| !e.skip_worktree).cloned().collect();
            let start = index.iter().filter(|e| e.stage == 0).map(|e| (e.path.clone(), (e.hash, e.mode))).collect();
            // Unmerged paths with both sides: a combined diff when a patch is shown.
            let combined = opts.patch && opts.unmerged_stage.is_none();
            if combined && !opts.reverse {
                combined_text = crate::helpers::combine_diff::combined_worktree_diffs(&tracked, &specs)?;
            }
            (diff_files_changes(&tracked, &specs, opts.unmerged_stage.unwrap_or(2), combined, true), start)
        }
        (true, 0) | (true, 1) => {
            let tree = match revs.first() {
                Some(rev) => tree_of(rev),
                None if refs::resolve_head_commit()?.is_some() => tree_of("HEAD"),
                None => TreeMap::new(),
            };
            (diff_index_changes(&tree, &index, true, false, true, &specs), tree)
        }
        (false, 1) => {
            let tree = tree_of(&revs[0]);
            let mut changes = diff_index_changes(&tree, &index, false, false, true, &specs);
            // A file left out by a sparse checkout stands for its staged content.
            for change in changes.iter_mut().filter(|c| skipped.contains(&c.path)) {
                change.new = index.iter().find(|e| e.path == change.path).map(|e| DiffSide { mode: e.mode, hash: e.hash, worktree: false });
            }
            changes.retain(|c| c.old != c.new);
            (changes, tree)
        }
        (false, 2) => {
            let (a, b) = (tree_hash_of(&revs[0]), tree_hash_of(&revs[1]));
            let mut changes = Vec::new();
            diff_trees(Some(&a), Some(&b), "", &opts, &specs, &mut changes)?;
            (changes, tree_of(&revs[0]))
        }
        _ => die(&usage, 129),
    };
    if opts.reverse {
        reverse_changes(&mut changes);
    }
    if opts.renames.level > 0 {
        crate::helpers::diff_plumbing::fill_rename_destination_hashes(&mut changes);
        let mut start = if opts.renames.find_copies_harder && !opts.reverse { start } else { TreeMap::new() };
        start.retain(|p, _| crate::helpers::diff_plumbing::diff_path_selected(p, false, &specs));
        let unmodified = unmodified_files(&start, &changes);
        changes = detect_renames(changes, &opts.renames, &unmodified);
    }
    if !(opts.quiet || opts.no_output) {
        std::io::stdout().lock().write_all(combined_text.as_bytes())?;
    }
    std::io::stdout().lock().write_all(render_diff_changes(&changes, &opts)?.as_bytes())?;
    warn_rename_limit("diff.renameLimit");
    if (opts.exit_code || opts.quiet) && (!changes.is_empty() || !combined_text.is_empty()) {
        std::process::exit(1);
    }
    Ok(())
}
