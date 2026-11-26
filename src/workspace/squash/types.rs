/// The commits `rgit squash <n>` folds together, oldest first, and where the result goes.
// Used by: src/workspace/squash.rs, src/workspace/squash/engine.rs
// Used by command/helper: `rgit squash` (`plan_squash`, `combined_message`)
#[derive(Debug, Clone)]
pub struct SquashPlan {
    /// The commits, oldest first; the last one is HEAD.
    pub commits: Vec<crate::core::types::ParsedCommit>,
    /// The parent of the oldest one, which the squashed commit gets (`None` for the root).
    pub base: Option<String>,
}
