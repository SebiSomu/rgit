//! Types of `rgit stats`.

/// One commit as `rgit stats` counts it: hash, subject, author, author time (UTC) and the
/// author's time-zone offset in seconds, the lines it added and removed and the files it
/// changed (against its first parent).
// Used by: src/history/stats/types.rs, src/history/stats/engine.rs, src/history/stats.rs
// Used by command/helper: `collect_commits` and the `stats` command
#[derive(Debug, Clone)]
pub struct CommitStat {
    pub hash: String,
    pub subject: String,
    pub author: String,
    pub timestamp: i64,
    pub offset: i64,
    pub added: usize,
    pub removed: usize,
    pub files: usize,
}

/// One author's totals: commits, lines added and removed, first and last commit time.
// Used by: src/history/stats/types.rs, src/history/stats.rs
// Used by command/helper: the `stats` command
#[derive(Debug, Clone, Default)]
pub struct AuthorStat {
    pub commits: usize,
    pub added: usize,
    pub removed: usize,
    pub first: i64,
    pub last: i64,
}
