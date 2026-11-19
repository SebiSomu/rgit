use crate::merge::{merge::types::MergeFavor, types::MergeConflictStyle};

/// Settings of a line-level three-way file merge (git's `xmparam_t`).
// Used by: src/merge/merge_file/types.rs, src/merge/merge_file.rs, src/merge/merge_tree/engine.rs, src/workspace/restore.rs
// Used by command/helper: `merge_file_lines_with`, `parse_merge_file_args`, and the `merge_file` command
#[derive(Debug, Clone)]
pub struct MergeFileOptions {
    /// Label after `<<<<<<<`.
    pub ours_label: String,
    /// Label after `|||||||` (diff3 styles only).
    pub base_label: String,
    /// Label after `>>>>>>>`.
    pub theirs_label: String,
    /// Length of the conflict markers (git's default is 7).
    pub marker_size: usize,
    /// `merge`, `diff3` or `zdiff3`.
    pub style: MergeConflictStyle,
    /// Which side wins a conflict, if any.
    pub favor: MergeFavor,
    /// Also join conflicts separated only by lines without letters or digits (`merge-file`).
    pub join_non_alnum: bool,
}

impl Default for MergeFileOptions {
    fn default() -> Self {
        MergeFileOptions { ours_label: String::new(), base_label: String::new(), theirs_label: String::new(), marker_size: 7, style: MergeConflictStyle::Merge, favor: MergeFavor::None, join_non_alnum: false }
    }
}

/// Parsed arguments of the `merge-file` command.
// Used by: src/merge/merge_file/engine.rs
// Used by command/helper: `parse_merge_file_args` and the `merge_file` command
#[derive(Debug, Clone)]
pub struct MergeFileArgs {
    /// `-p` / `--stdout`: print the result instead of overwriting the first file.
    pub stdout: bool,
    /// `--object-id`: the three arguments are blobs, and the result is written as a blob.
    pub object_id: bool,
    /// `-q` / `--quiet`: no warnings about conflicts.
    pub quiet: bool,
    /// `--diff3` / `--zdiff3`, else `merge.conflictStyle`.
    pub style: MergeConflictStyle,
    /// `--ours` / `--theirs` / `--union`.
    pub favor: MergeFavor,
    /// `--marker-size=<n>`.
    pub marker_size: usize,
    /// Up to three `-L <label>` values: current, base, other.
    pub labels: Vec<String>,
    /// `<current> <base> <other>`.
    pub files: Vec<String>,
}
