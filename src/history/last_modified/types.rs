use crate::history::rev_list::types::RevListOptions;

/// Parsed arguments for the `last-modified` command, which shows the commit that last
/// changed each path.
// Used by: src/history/last_modified/engine.rs
// Used by command/helper: `parse_last_modified_args`, `last_modified_paths`, and the `last_modified` command
#[derive(Debug, Clone, Default)]
pub struct LastModifiedOptions {
    /// `-r`: list every file instead of only the top-level entries.
    pub recursive: bool,
    /// `-t`: also list directories when recursing.
    pub show_trees: bool,
    /// `-z`: end entries with NUL instead of a newline.
    pub nul: bool,
    /// The revision range to search and the paths to report.
    pub revs: RevListOptions,
}
