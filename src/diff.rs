pub mod combine;
// The `diff` command is named after its domain (`diff/diff.rs`), by convention.
#[allow(clippy::module_inception)]
pub mod diff;
pub mod diff_files;
pub mod diff_index;
pub mod diff_tree;
pub mod hunks;
pub mod myers;
pub mod patch_id;
pub mod plumbing;
pub mod rename;
pub mod types;
