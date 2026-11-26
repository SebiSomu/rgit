/// Where a piece of lost work was found, which also says how it got lost.
// Used by: src/workspace/recover.rs, src/workspace/recover/engine.rs
// Used by command/helper: `rgit recover` (listing and restoring)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LostKind {
    /// A stash (or `rgit wip` save) that was dropped or cleared.
    Stash,
    /// A commit a ref moved away from, still in that ref's reflog: `entry` is the reflog
    /// line that names it, e.g. `main@{2}: reset: moving to HEAD~3`.
    LeftBehind { entry: String },
    /// A commit nothing points at any more, not even a reflog (a deleted branch's reflog goes
    /// with it, an expired reflog, an object from another tool).
    Dangling,
}

/// One piece of lost work: the newest commit of a line of history that nothing keeps.
// Used by: src/workspace/recover.rs, src/workspace/recover/engine.rs
// Used by command/helper: `rgit recover`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LostWork {
    pub commit: String,
    pub kind: LostKind,
    pub subject: String,
    /// Committer time and timezone (`+0200`) of the commit.
    pub time: i64,
    pub tz: String,
    /// How many commits would come back with it (it and its lost ancestors).
    pub commits: usize,
}

/// What `find_lost_work` needs to know about one commit.
// Used by: src/workspace/recover/engine.rs
// Used by command/helper: `find_lost_work`, `lost_tips` (and their unit tests)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitFacts {
    pub parents: Vec<String>,
    pub time: i64,
    pub tz: String,
    pub subject: String,
}
