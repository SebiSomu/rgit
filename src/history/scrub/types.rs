/// One ref `rgit scrub` moves: a branch or tag, from its old object to the rewritten one.
// Used by: src/history/scrub.rs, src/history/scrub/engine.rs
// Used by command/helper: `rgit scrub` (`plan_scrub`, `apply_scrub`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefChange {
    pub name: String,
    pub old: String,
    pub new: String,
}

/// What a scrub does: the paths it removes, the refs it moves, and how much history changes.
// Used by: src/history/scrub.rs, src/history/scrub/engine.rs
// Used by command/helper: `rgit scrub` (`plan_scrub`, the report, `apply_scrub`)
#[derive(Debug, Clone, Default)]
pub struct ScrubPlan {
    /// The paths, `/`-separated and relative to the top of the repository.
    pub paths: Vec<String>,
    /// Branches and tags that change (others already don't contain the paths).
    pub refs: Vec<RefChange>,
    /// Commits looked at (everything the branches and tags reach).
    pub commits: usize,
    /// Commits whose files included one of the paths.
    pub containing: usize,
    /// Commits written again (those, and every commit after them).
    pub rewritten: usize,
    /// Commits left out because removing the paths left them with no change of their own.
    pub dropped: usize,
    /// Old commit -> new commit, for every commit that changed (a dropped one maps to its parent).
    pub mapping: std::collections::HashMap<String, String>,
}
