use crate::merge::merge_tree::types::{MergeMessage, MergePathReport};

/// The full result of an in-memory tree merge.
// Used by: src/merge/merge.rs, src/merge/merge/engine.rs, src/merge/merge_tree.rs, src/merge/merge_tree/engine.rs
// Used by command/helper: `merge_trees_in_memory` and the `merge_tree` command
#[derive(Debug, Clone, Default)]
pub struct TreeMergeResult {
    /// The merged tree as a flat path → (blob hash, mode) map; conflicted files hold marker content.
    pub tree: std::collections::BTreeMap<String, ([u8; 20], u32)>,
    /// Conflicted paths with their base (stage 1), ours (stage 2), and theirs (stage 3) entries.
    pub conflicted: std::collections::BTreeMap<String, [Option<([u8; 20], u32)>; 3]>,
    pub messages: Vec<MergeMessage>,
    pub reports: Vec<MergePathReport>,
}

/// How conflicts are written by a file merge.
// Used by: src/merge/merge_file.rs, src/merge/merge_file/engine.rs, src/merge/merge_file/types.rs, src/merge/merge_tree/engine.rs, src/workspace/restore.rs, src/workspace/restore/types.rs
// Used by command/helper: `merge_file_lines_with`, `parse_merge_file_args`, and the `merge_file` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeConflictStyle {
    /// Ours and theirs only, conflicts narrowed down as far as possible.
    Merge,
    /// Whole conflicts with the base version in the middle.
    Diff3,
    /// Like `Diff3`, but lines both sides share at the edges are moved out of the conflict.
    Zdiff3,
}
