//! Types of `rgit compare`.

/// How a file changed on one side since the merge base.
// Used by: src/history/compare/types.rs, src/history/compare/engine.rs, src/history/compare.rs
// Used by command/helper: `side_change`, `file_differences` and the `compare` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Same,
    Added,
    Deleted,
    Changed,
}

impl Change {
    /// `added` / `deleted` / `changed`.
    pub fn verb(self) -> &'static str {
        match self {
            Change::Same => "unchanged",
            Change::Added => "added",
            Change::Deleted => "deleted",
            Change::Changed => "changed",
        }
    }
}

/// A file whose content differs between the two tips, with how each side changed it since the
/// merge base.
// Used by: src/history/compare/types.rs, src/history/compare/engine.rs, src/history/compare.rs
// Used by command/helper: `file_differences` and the `compare` command
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub left: Change,
    pub right: Change,
}
