use crate::merge::types::MergeConflictStyle;

/// Which version of an unmerged path `restore` / `checkout -- <paths>` write to the working tree.
// Used by: src/workspace/restore.rs
// Used by command/helper: `writeout_stage`, `restore`, `checkout_paths`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteoutStage {
    /// The index entry; an unmerged path is an error.
    Index,
    /// `--ours`: stage 2.
    Ours,
    /// `--theirs`: stage 3.
    Theirs,
    /// `--merge` / `--conflict=<style>`: stages 1-3 merged again, with conflict markers.
    Merge(MergeConflictStyle),
}
