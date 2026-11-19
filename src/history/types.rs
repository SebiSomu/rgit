use crate::core::types::ParsedCommit;

/// A commit visited by the `rev-list` walk: its parsed object, the parents the walk followed
/// (simplified when paths limit history), whether history simplification hides it, and
/// whether it belongs to the left side of a symmetric range.
// Used by: src/history/log.rs, src/history/rev_list.rs, src/history/rev_list/engine.rs
// Used by command/helper: `run_rev_list_walk`, `sort_rev_list_topologically`, `rewrite_rev_list_parents`, and the `rev_list` command
#[derive(Debug, Clone, Default)]
pub struct WalkedCommit {
    pub commit: ParsedCommit,
    pub parents: Vec<String>,
    pub hidden: bool,
    pub left: bool,
}
