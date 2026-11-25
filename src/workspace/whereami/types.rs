//! Types of `rgit whereami`.

/// Where the current branch stands against its upstream, from the remote-tracking ref as of
/// the last fetch.
// Used by: src/workspace/whereami/types.rs, src/workspace/whereami/engine.rs, src/workspace/whereami.rs
// Used by command/helper: `upstream`, `next_step` and the `whereami` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upstream {
    /// No upstream configured; the remote it would be pushed to, if the repository has one.
    None(Option<String>),
    /// Configured, but the remote-tracking ref is gone (deleted on the remote and pruned).
    Gone(String),
    /// `origin/main`, commits only here, commits only there.
    Tracking { name: String, ahead: usize, behind: usize },
}

/// Everything `rgit whereami` reports, read once.
// Used by: src/workspace/whereami/types.rs, src/workspace/whereami/engine.rs, src/workspace/whereami.rs
// Used by command/helper: `gather`, `next_step` and the `whereami` command
#[derive(Debug, Clone)]
pub struct Situation {
    /// Current branch; None when HEAD is detached.
    pub branch: Option<String>,
    /// HEAD's commit; None on a branch with no commits yet.
    pub head: Option<String>,
    pub upstream: Upstream,
    pub staged: usize,
    pub modified: usize,
    pub untracked: usize,
    pub conflicted: usize,
    /// `rebase`, `merge`, `cherry-pick`, `revert`, `bisect`, `am` or `stash` (an apply that
    /// stopped on conflicts).
    pub operation: Option<String>,
    pub stash_count: usize,
    /// `rgit wip` saves on the current branch.
    pub wip_count: usize,
}
