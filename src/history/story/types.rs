//! Types of `rgit story`.

/// One commit in a path's history, as `rgit story` tells it: the commit, its author, time and
/// subject, the lines it added and removed (None for binary files), the line count of the
/// path before it, and whether it created or deleted the path or renamed it (from where).
// Used by: src/history/story/types.rs, src/history/story/engine.rs, src/history/story.rs
// Used by command/helper: `walk_history` and the `story` command
#[derive(Debug, Clone)]
pub struct Event {
    pub commit: String,
    pub author: String,
    pub timestamp: i64,
    pub subject: String,
    pub added: Option<usize>,
    pub removed: Option<usize>,
    pub old_lines: usize,
    pub created: bool,
    pub deleted: bool,
    /// `(old path, new path)` for a rename or copy detected in this commit.
    pub renamed: Option<(String, String)>,
}
