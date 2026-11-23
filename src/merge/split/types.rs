//! Types of `rgit split`.

use std::collections::BTreeMap;

use crate::workspace::absorb::types::StagedHunk;

/// A path's blob and mode in a tree.
pub type PathMap = BTreeMap<String, ([u8; 20], u32)>;

/// One piece of the commit being split, the unit the user assigns to a new commit: a hunk of a
/// text file (no context lines), or a whole-file change (added, deleted, binary, mode change)
/// with the file's state in the commit (`None` = deleted) and what kind of change it is.
// Used by: src/merge/split/types.rs, src/merge/split/engine.rs, src/merge/split.rs
// Used by command/helper: `pieces_of`, `show_piece`, `tree_with` and the `split` command
#[derive(Debug, Clone)]
pub enum Piece {
    Hunk(StagedHunk),
    File { path: String, new: Option<([u8; 20], u32)>, what: String },
}

/// The commit being split, taken apart: its parent's and its own flattened trees, the parent's
/// lines of every file that has hunks, and the pieces in path order.
// Used by: src/merge/split/types.rs, src/merge/split/engine.rs, src/merge/split.rs
// Used by command/helper: `pieces_of`, `show_piece`, `tree_with` and the `split` command
#[derive(Debug)]
pub struct SplitSource {
    pub parent_tree: PathMap,
    pub commit_tree: PathMap,
    pub parent_lines: BTreeMap<String, Vec<String>>,
    pub pieces: Vec<Piece>,
}
