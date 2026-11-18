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
/// `--exit-code`). An unmerged path shows as `U` (`* Unmerged path` in a patch, `Unmerged` in
/// `--stat`), then its stage 2 (`-1`/`-3`: stage 1/3) compared with the working tree; with a
/// patch and no stage chosen, a path with both sides gets a combined diff (`diff --cc`)
/// instead. Unlike git, a file whose cached stat data is stale but whose content is
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
    let index = read_index()?;
    // With a patch and no stage chosen, unmerged paths that have both sides get a combined
    // diff, printed before everything else (git shows them as it walks the index).
    let combined = opts.patch && opts.unmerged_stage.is_none();
    let combined_text = if combined && !opts.reverse { crate::helpers::combine_diff::combined_worktree_diffs(&index, &specs)? } else { String::new() };
    let mut changes = diff_files_changes(&index, &specs, opts.unmerged_stage.unwrap_or(2), combined, false);
    if opts.reverse {
        reverse_changes(&mut changes);
    }
    if opts.renames.level > 0 {
        let mut start: std::collections::BTreeMap<String, ([u8; 20], u32)> = std::collections::BTreeMap::new();
        if opts.renames.find_copies_harder && !opts.reverse {
            start = index.iter().filter(|e| e.stage == 0).map(|e| (e.path.clone(), (e.hash, e.mode))).collect();
            start.retain(|p, _| crate::helpers::diff_plumbing::diff_path_selected(p, false, &specs));
        }
        let unmodified = crate::helpers::diff_plumbing::unmodified_files(&start, &changes);
        changes = crate::helpers::rename::detect_renames(changes, &opts.renames, &unmodified);
    }
    if !(opts.quiet || opts.no_output) {
        std::io::stdout().lock().write_all(combined_text.as_bytes())?;
    }
    std::io::stdout().lock().write_all(render_diff_changes(&changes, &opts)?.as_bytes())?;
    crate::helpers::rename::warn_rename_limit("diff.renameLimit");
    if (opts.exit_code || opts.quiet) && (!changes.is_empty() || !combined_text.is_empty()) {
        std::process::exit(1);
    }
    Ok(())
}
