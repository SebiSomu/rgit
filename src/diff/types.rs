/// Represents an atomic edit operation emitted during Myers line-by-line diff calculation.
// Used by: src/diff/hunks.rs, src/diff/myers.rs, src/history/blame/engine.rs
// Used by command/helper: `myers_diff`, `format_diff_output`, and the `diff` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffOp {
    Keep(String),
    Delete(String),
    Insert(String),
}

/// One file's contribution to a `--stat` block: display name, added/deleted line counts,
/// and `(old_size, new_size)` in bytes for binary files.
// Used by: src/diff/types.rs, src/diff/hunks.rs, src/diff/plumbing.rs, src/history/follow.rs
// Used by command/helper: `diff_tree_file` and `format_diffstat`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffStatEntry {
    pub name: String,
    pub added: usize,
    pub deleted: usize,
    pub binary_sizes: Option<(usize, usize)>,
    /// An unmerged path: shown as `Unmerged`, not counted as a changed file.
    pub unmerged: bool,
}

/// The git-format diff of one path between two trees: the full `diff --git` section, its
/// diffstat entry, and its `--summary` line (creation, deletion, or mode change), if any.
// Used by: src/diff/hunks.rs, src/diff/plumbing.rs, src/mail/request_pull/engine.rs
// Used by command/helper: `diff_tree_file`, `diff_commit_against_parent`, and the `format_patch` command
#[derive(Debug, Clone, Default)]
pub struct FileDiff {
    pub text: String,
    pub stat: DiffStatEntry,
    pub summary: Option<String>,
}

/// One side of a raw diff entry: the mode and object of a path in a tree, the index or the
/// working tree.
// Used by: src/diff/types.rs, src/diff/diff.rs, src/diff/plumbing.rs, src/diff/rename.rs, src/export/fast_export/engine.rs, src/history/log/engine.rs, src/workspace/staging.rs
// Used by command/helper: `diff_trees`, `diff_index_changes`, `diff_files_changes`, `render_diff_changes`, and the `diff_tree` / `diff_index` / `diff_files` commands
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSide {
    /// File mode (`100644`, `100755`, `120000`, `160000`, or `040000` for a tree).
    pub mode: u32,
    /// Object hash; all zeros when the content is only in the working tree.
    pub hash: [u8; 20],
    /// True when the content has to be read from the working tree file.
    pub worktree: bool,
}

/// One changed path of a raw diff.
// Used by: src/diff/diff.rs, src/diff/plumbing.rs, src/diff/rename.rs, src/export/fast_export/engine.rs, src/history/log/engine.rs, src/workspace/staging.rs
// Used by command/helper: `diff_trees`, `diff_index_changes`, `diff_files_changes`, `render_diff_changes`, and the `diff_tree` / `diff_index` / `diff_files` commands
#[derive(Debug, Clone, Default)]
pub struct DiffChange {
    /// The path (the new path of a rename or copy).
    pub path: String,
    /// The path before; `None` for an addition.
    pub old: Option<DiffSide>,
    /// The path after; `None` for a deletion.
    pub new: Option<DiffSide>,
    /// For a detected rename or copy: the source path, `R` or `C`, and git's 0..60000 score.
    pub renamed_from: Option<(String, char, u32)>,
    /// An unmerged path (status `U`): `* Unmerged path` in a patch, `Unmerged` in `--stat`.
    pub unmerged: bool,
}
