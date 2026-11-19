pub mod banana_split;
pub mod cherry_pick;
pub mod conflicts;
pub mod fmt_merge_msg;
// The `merge` command is named after its domain (`merge/merge.rs`), by convention.
#[allow(clippy::module_inception)]
pub mod merge;
pub mod merge_file;
pub mod merge_tree;
pub mod rebase;
pub mod replay;
pub mod rerere;
pub mod revert;
pub mod sequencer;
