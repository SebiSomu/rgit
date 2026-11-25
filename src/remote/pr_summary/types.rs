//! Types of `rgit pr-summary`.

/// One file the pull request changes: path, whether it is new or deleted, and its lines added
/// and removed (none for binary files).
// Used by: src/remote/pr_summary/types.rs, src/remote/pr_summary/engine.rs, src/remote/pr_summary.rs
// Used by command/helper: `changed_files` and the `pr_summary` command
#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: String,
    pub new: bool,
    pub deleted: bool,
    pub binary: bool,
    pub added: usize,
    pub removed: usize,
}
