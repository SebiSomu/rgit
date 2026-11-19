/// `merge --ff` (default), `--no-ff` or `--ff-only`; the last one given wins.
// Used by: src/merge/merge/types.rs, src/cli.rs, src/merge/merge.rs
// Used by command/helper: the `merge` command
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FastForwardMode {
    #[default]
    Allow,
    Never,
    Only,
}

/// Parsed arguments of `rgit merge`.
// Used by: src/cli.rs, src/merge/merge.rs, src/remote/pull.rs
// Used by command/helper: the `merge` and `pull` commands
#[derive(Debug, Clone, Default)]
pub struct MergeOptions {
    /// The commit to merge (one; octopus merges are not supported).
    pub commits: Vec<String>,
    pub ff: FastForwardMode,
    /// `--squash`: merge into the index and working tree only, then stop.
    pub squash: bool,
    /// `--commit` (Some(true)) / `--no-commit` (Some(false)); unset is "commit".
    pub commit: Option<bool>,
    /// `-m` values, joined as paragraphs.
    pub messages: Vec<String>,
    /// `-F <file>`.
    pub file: Option<String>,
    /// `-s <strategy>`: ort (default), recursive or ours.
    pub strategy: Option<String>,
    /// `-X <option>`: ours or theirs.
    pub strategy_options: Vec<String>,
    pub abort: bool,
    pub cont: bool,
    pub quit: bool,
    /// `-n` / `--no-stat`: no diffstat after the merge.
    pub no_stat: bool,
    pub quiet: bool,
    pub allow_unrelated: bool,
}

/// The side a file merge takes for conflicting regions instead of writing markers.
// Used by: src/merge/merge.rs, src/merge/merge/engine.rs, src/merge/merge_file/engine.rs, src/merge/merge_file/types.rs, src/merge/merge_tree/engine.rs
// Used by command/helper: `merge_file_lines_with`, `parse_merge_file_args`, and the `merge_file` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeFavor {
    None,
    Ours,
    Theirs,
    Union,
}
