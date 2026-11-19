/// Encapsulates a parsed gitignore glob pattern rule and its directory context.
// Used by: src/workspace/check_ignore/types.rs, src/workspace/clean.rs, src/workspace/clean/engine.rs, src/workspace/gitignore.rs, src/workspace/ls_files/engine.rs
// Used by command/helper: `parse_gitignore_line`, `is_path_ignored`, `collect_clean_candidates`, and the `clean` command
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitIgnoreRule {
    pub pattern: String,
    pub base_dir: String,
    pub negated: bool,
    pub dir_only: bool,
    pub has_slash: bool,
}

/// What the working tree holds for an index entry.
// Used by: src/diff/plumbing.rs, src/workspace/checkout_index.rs, src/workspace/index_plumbing.rs, src/workspace/read_tree/engine.rs, src/workspace/reset.rs, src/workspace/sparse_checkout/engine.rs
// Used by command/helper: `worktree_state`, `refresh_index`, `diff_index_changes`, and the `update_index` / `checkout_index` / `read_tree` / `diff_files` / `diff_index` commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexFileState {
    /// No file at the entry's path.
    Missing,
    /// The file has the entry's content and mode.
    Clean,
    /// The file differs; the value is the mode it would be staged with.
    Modified(u32),
}
