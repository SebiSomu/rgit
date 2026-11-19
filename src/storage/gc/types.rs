use crate::core::types::LooseObject;

/// What a `gc` run found: how many loose objects exist, how many are reachable, which
/// unreachable ones were (or, in a dry run, would be) pruned, and how many reflog entries
/// expired.
// Used by: src/storage/gc/types.rs, src/storage/gc/engine.rs
// Used by command/helper: `plan_gc` and the `gc` command
#[derive(Debug, Clone, Default)]
pub struct GcReport {
    /// Loose objects in the repository.
    pub total_objects: usize,
    /// Of those, the ones reachable from a ref, HEAD, the index, the stash, an operation in progress or a live reflog entry.
    pub reachable_objects: usize,
    /// Of those, the ones nothing leads to.
    pub unreachable_objects: usize,
    /// Unreachable objects that were spared: too new, referenced by a too-new object, or pruning is off.
    pub kept_unreachable: usize,
    /// Unreachable objects that were (or would be) deleted.
    pub pruned: Vec<LooseObject>,
    /// Reflog entries that were (or would be) expired.
    pub reflog_entries_expired: usize,
}

impl GcReport {
    /// Total size in bytes of the objects that were (or would be) pruned.
    pub fn pruned_bytes(&self) -> u64 {
        self.pruned.iter().map(|o| o.size).sum()
    }
}
