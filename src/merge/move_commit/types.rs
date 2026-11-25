//! Types of `rgit move-commit`.

/// The outcome of replaying commits in memory onto a new base: the new tip, or the first
/// commit that doesn't apply cleanly and the paths it conflicts in.
// Used by: src/merge/move_commit/types.rs, src/merge/move_commit/engine.rs, src/merge/move_commit.rs
// Used by command/helper: `replay_in_memory` and the `move_commit` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replayed {
    Done(String),
    Conflict { commit: String, paths: Vec<String> },
}
