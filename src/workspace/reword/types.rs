/// The commits `rgit reword` rewrites: the one whose message changes, then every commit after
/// it up to HEAD on HEAD's first-parent line (they only get new parents).
// Used by: src/workspace/reword.rs, src/workspace/reword/engine.rs
// Used by command/helper: `rgit reword` (`reword_chain`, `rewrite_chain`)
#[derive(Debug, Clone)]
pub struct RewordChain {
    /// Oldest first: `commits[0]` is the commit to reword, the last one is HEAD.
    pub commits: Vec<crate::core::types::ParsedCommit>,
}
