use std::io::Write;
use anyhow::Result;

use crate::helpers::diff_plumbing::{diff_files_changes, parse_diff_plumbing_args, render_diff_changes, reverse_changes, DIFF_COMMON_OPTIONS};
use crate::helpers::ls_files::normalize_pathspecs;
use crate::index::read_index;
use crate::repo;

/// `rgit diff-files [-q] [<common diff options>] [<path>...]`
///
/// Compares the index with the working tree: every tracked file that was edited
/// (`M`, new side shown with an all-zero hash because the content is not in the object
/// store), had its executable bit changed, or was removed (`D`). Untracked files are not
/// reported. Output formats are those of `diff-tree` (raw by default, `-p`, `--name-only`,
/// `--name-status`, `--stat`, `--numstat`, `--summary`, `-z`, `--abbrev`, `-R`, `--quiet`,
/// `--exit-code`). Unlike git, a file whose cached stat data is stale but whose content is
/// unchanged counts as unchanged. Exit status: 0 (1 with `--exit-code` and differences),
/// 128 on errors, 129 for a usage error.
pub fn diff_files(args: Vec<String>) -> Result<()> {
    let usage = format!("usage: rgit diff-files [-q] [<common-diff-options>] [<path>...]\n\n{}", DIFF_COMMON_OPTIONS);
    let mut opts = match parse_diff_plumbing_args(&args, "diff-files", &usage) {
        Ok(opts) => opts,
        Err((message, code)) => {
            if code == 129 && message.starts_with("usage:") {
                print!("{}", message);
            } else {
                eprint!("{}", message);
            }
            std::process::exit(code);
        }
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        std::process::exit(128);
    }
    let mut paths = std::mem::take(&mut opts.args);
    paths.extend(std::mem::take(&mut opts.paths));
    let specs = normalize_pathspecs(&paths);
    let mut changes = diff_files_changes(&read_index()?, &specs);
    if opts.reverse {
        reverse_changes(&mut changes);
    }
    std::io::stdout().lock().write_all(render_diff_changes(&changes, &opts)?.as_bytes())?;
    if (opts.exit_code || opts.quiet) && !changes.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}
