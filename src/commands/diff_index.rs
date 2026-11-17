use std::collections::BTreeMap;
use std::io::Write;
use anyhow::Result;

use crate::helpers::diff_plumbing::{check_path_argument, diff_index_changes, parse_diff_plumbing_args, render_diff_changes, reverse_changes, DIFF_COMMON_OPTIONS};
use crate::helpers::ls_files::normalize_pathspecs;
use crate::helpers::objects::flatten_tree;
use crate::helpers::rev_parse::{peel_to_type, resolve_rev};
use crate::index::read_index;
use crate::repo;

/// `rgit diff-index [-m] [--cached] [<common diff options>] <tree-ish> [<path>...]`
///
/// Compares a tree with the index (`--cached`) or with the working tree. Without
/// `--cached`, each tracked file is looked at in the working tree: if it still matches the
/// index, the index entry is compared with the tree; if it was edited, it shows as changed
/// with an all-zero hash for the new side (its content is not in the object store); if it
/// is gone, it shows as deleted (`-m`: as unchanged). Untracked files are never reported.
/// Output formats are those of `diff-tree` (raw by default, `-p`, `--name-only`,
/// `--name-status`, `--stat`, `--numstat`, `--summary`, `-z`, `--abbrev`, `-R`, `--quiet`,
/// `--exit-code`). Unlike git, a file whose cached stat data is stale but whose content is
/// unchanged counts as unchanged, so no `update-index --refresh` is needed first.
/// Exit status: 0 (1 with `--exit-code` and differences), 128 on errors, 129 for a usage
/// error.
pub fn diff_index(args: Vec<String>) -> Result<()> {
    let usage = format!("usage: rgit diff-index [-m] [--cached] [<common-diff-options>] <tree-ish> [<path>...]\n\n{}", DIFF_COMMON_OPTIONS);
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        std::process::exit(code);
    };
    let mut opts = match parse_diff_plumbing_args(&args, "diff-index", &usage) {
        Ok(opts) => opts,
        Err((message, code)) => {
            if code == 129 && message.starts_with("usage:") {
                print!("{}", message);
                std::process::exit(code);
            }
            die(&message, code)
        }
    };
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git\n", 128);
    }
    if opts.args.is_empty() {
        die(&usage, 129);
    }
    let rev = opts.args.remove(0);
    let Ok(hash) = resolve_rev(&rev) else {
        die(&format!("fatal: ambiguous argument '{}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'\n", rev), 128)
    };
    let Ok(tree) = peel_to_type(hash, "tree") else { die(&format!("error: bad tree object {}\n", rev), 128) };
    let mut tree_map = BTreeMap::new();
    flatten_tree(&tree, "", &mut tree_map)?;

    let mut paths = std::mem::take(&mut opts.args);
    for path in &paths {
        if let Err(message) = check_path_argument(path) {
            die(&message, 128);
        }
    }
    paths.extend(std::mem::take(&mut opts.paths));
    let specs = normalize_pathspecs(&paths);
    let mut changes = diff_index_changes(&tree_map, &read_index()?, opts.cached, opts.merges, &specs);
    if opts.reverse {
        reverse_changes(&mut changes);
    }
    if opts.renames.level > 0 {
        let mut start = if opts.renames.find_copies_harder && !opts.reverse { tree_map.clone() } else { BTreeMap::new() };
        start.retain(|p, _| crate::helpers::diff_plumbing::diff_path_selected(p, false, &specs));
        let unmodified = crate::helpers::diff_plumbing::unmodified_files(&start, &changes);
        changes = crate::helpers::rename::detect_renames(changes, &opts.renames, &unmodified);
    }
    std::io::stdout().lock().write_all(render_diff_changes(&changes, &opts)?.as_bytes())?;
    crate::helpers::rename::warn_rename_limit("diff.renameLimit");
    if (opts.exit_code || opts.quiet) && !changes.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
