use crate::diff::rename::types::RenameOptions;

/// Options shared by `diff-tree`, `diff-index` and `diff-files`.
// Used by: src/diff/diff_tree.rs, src/diff/plumbing.rs, src/history/log/engine.rs, src/merge/merge.rs, src/workspace/commit_summary.rs
// Used by command/helper: `parse_diff_plumbing_args`, `render_diff_changes`, and the `diff_tree` / `diff_index` / `diff_files` commands
#[derive(Debug, Clone, Default)]
pub struct DiffPlumbingOptions {
    /// `-1`/`--base`, `-2`/`--ours`, `-3`/`--theirs` (`diff-files`, `diff`): the stage of an
    /// unmerged path compared with the working tree; with none, a patch shows a combined diff.
    pub unmerged_stage: Option<u8>,
    /// `--cc`: combined diffs for unmerged paths even without a patch format asked for.
    pub combined: bool,
    /// `--raw` (the default when no other format is asked for).
    pub raw: bool,
    /// `-p` / `-u` / `--patch`.
    pub patch: bool,
    /// `--name-only`.
    pub name_only: bool,
    /// `--name-status`.
    pub name_status: bool,
    /// `--stat`.
    pub stat: bool,
    /// `--shortstat`.
    pub shortstat: bool,
    /// `--numstat`.
    pub numstat: bool,
    /// `--summary`.
    pub summary: bool,
    /// `-s` / `--no-patch`: no diff output at all.
    pub no_output: bool,
    /// `-z`: NUL-terminated records, paths never quoted.
    pub nul: bool,
    /// `--abbrev[=<n>]`: shorten hashes in raw output.
    pub abbrev: Option<usize>,
    /// `-M` / `-C` / `--find-copies-harder` / `-l<n>` / `--no-renames`.
    pub renames: RenameOptions,
    /// Width of the `--stat` block (0: 80 columns).
    pub stat_width: usize,
    /// `-r`: recurse into subtrees (`diff-tree`).
    pub recursive: bool,
    /// `-t`: also show the tree entries themselves (`diff-tree`; implies `-r`).
    pub show_trees: bool,
    /// `--root`: show a root commit as a diff against nothing (`diff-tree`).
    pub root: bool,
    /// `-m`: show merges against each parent (`diff-tree`); treat files missing from the
    /// working tree as unchanged (`diff-index`).
    pub merges: bool,
    /// `--cached`: compare the tree with the index only (`diff-index`).
    pub cached: bool,
    /// `--stdin`: read commits or tree pairs from standard input (`diff-tree`).
    pub stdin: bool,
    /// `--no-commit-id`: do not print the commit id line (`diff-tree`).
    pub no_commit_id: bool,
    /// `-v` / `--pretty[=<format>]` / `--format=<format>`: show the commit header (`diff-tree`).
    pub pretty: Option<String>,
    /// `-R`: swap the two sides.
    pub reverse: bool,
    /// `--exit-code`: exit 1 when there are differences.
    pub exit_code: bool,
    /// `--quiet`: no output, implies `--exit-code`.
    pub quiet: bool,
    /// Positional arguments before `--`.
    pub args: Vec<String>,
    /// Arguments after `--`: always paths.
    pub paths: Vec<String>,
}
