//! Types of `rgit timeline`.

/// One commit as the timeline shows it: subject, author, the author's local day
/// (`2023-11-13`) and weekday, the branch it is counted on, whether it is a merge (and of
/// which branch), and its lines added and removed (none for merges).
// Used by: src/history/timeline/types.rs, src/history/timeline/engine.rs, src/history/timeline.rs
// Used by command/helper: `collect_events` and the `timeline` command
#[derive(Debug, Clone)]
pub struct Event {
    pub subject: String,
    pub author: String,
    pub day: String,
    pub weekday: &'static str,
    /// Local days since 1970-01-01 (to find the quiet days between two shown ones).
    pub day_number: i64,
    pub branch: Option<String>,
    /// For a merge commit: the merged branch's name from the message (`feature/x`), or `a branch`.
    pub merged: Option<String>,
    pub added: usize,
    pub removed: usize,
}
