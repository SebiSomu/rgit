/// Options for the `merge-tree` command, which merges two commits in memory and writes only
/// the resulting tree, without touching the index, working tree, or refs.
// Used by: src/cli.rs, src/merge/merge_tree.rs
// Used by command/helper: the `merge_tree` command
#[derive(Debug, Clone, Default)]
pub struct MergeTreeOptions {
    /// `--name-only`: list only the names of conflicted paths, not their stages.
    pub name_only: bool,
    /// `--messages` / `--no-messages`: force the informational messages on or off (default: shown only on conflicts).
    pub messages: Option<bool>,
    /// `-z`: NUL-terminate output records and print messages in git's machine-readable form.
    pub nul: bool,
    /// `--merge-base=<tree-ish>`: use this as the merge base instead of computing it.
    pub merge_base: Option<String>,
    /// `--allow-unrelated-histories`: merge from an empty tree when the commits share no history.
    pub allow_unrelated_histories: bool,
    /// `--explain`: print a human-readable report of the simulated merge instead of git's output.
    pub explain: bool,
    pub branch1: String,
    pub branch2: String,
}

/// How one side of a merge changed a path relative to the merge base.
// Used by: src/merge/merge_tree/types.rs, src/merge/merge_tree.rs, src/merge/merge_tree/engine.rs
// Used by command/helper: `merge_trees_in_memory` and the `merge_tree --explain` report
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideChange {
    Unchanged,
    Added,
    Modified,
    Deleted,
}

impl SideChange {
    /// Classifies a side's entry against the base entry.
    pub fn between(base: Option<&([u8; 20], u32)>, side: Option<&([u8; 20], u32)>) -> SideChange {
        match (base, side) {
            (None, Some(_)) => SideChange::Added,
            (Some(_), None) => SideChange::Deleted,
            (Some(b), Some(s)) if b != s => SideChange::Modified,
            _ => SideChange::Unchanged,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SideChange::Unchanged => "-",
            SideChange::Added => "added",
            SideChange::Modified => "modified",
            SideChange::Deleted => "deleted",
        }
    }
}

/// What the in-memory merge decided for one path.
// Used by: src/merge/merge_tree/types.rs, src/merge/merge_tree.rs, src/merge/merge_tree/engine.rs
// Used by command/helper: `merge_trees_in_memory` and the `merge_tree --explain` report
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergePathOutcome {
    /// Both sides ended up with the same entry.
    SameOnBothSides,
    /// Only ours changed the path, so our version was kept.
    TakenFromOurs,
    /// Only theirs changed the path, so their version was taken.
    TakenFromTheirs,
    /// Both sides edited the file and the line-level merge combined them without conflicts.
    AutoMerged,
    /// The path conflicts; holds git's conflict kind (`content`, `add/add`, `modify/delete`, `binary`, `file/directory`, `mode`).
    Conflict(String),
}

/// One region of a file where both sides changed the same base lines differently.
// Used by: src/merge/merge_tree/types.rs, src/merge/merge_tree/engine.rs
// Used by command/helper: `merge_file_lines` and the `merge_tree --explain` conflict preview
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConflictRegion {
    /// 1-based first line of the region in the base file (the line before it for pure insertions).
    pub base_line: usize,
    pub base: Vec<String>,
    pub ours: Vec<String>,
    pub theirs: Vec<String>,
}

/// The result of a line-level three-way merge of one file.
// Used by: src/merge/merge_tree/types.rs, src/merge/merge_tree/engine.rs
// Used by command/helper: `merge_file_lines` and `merge_trees_in_memory`
#[derive(Debug, Clone, Default)]
pub struct FileMergeResult {
    /// The merged content, with conflict markers around each conflicting region.
    pub content: Vec<u8>,
    /// How many changed regions were taken from ours only, theirs only, or were identical on both.
    pub from_ours: usize,
    pub from_theirs: usize,
    pub from_both: usize,
    pub conflicts: Vec<ConflictRegion>,
}

/// The decision report for one path that at least one side changed.
// Used by: src/merge/merge_tree/engine.rs, src/merge/types.rs
// Used by command/helper: `merge_trees_in_memory` and the `merge_tree --explain` report
#[derive(Debug, Clone)]
pub struct MergePathReport {
    pub path: String,
    pub ours: SideChange,
    pub theirs: SideChange,
    pub outcome: MergePathOutcome,
    /// Line-merge statistics when both sides edited the file's text.
    pub file: Option<FileMergeResult>,
}

/// One informational message of a merge, as git's `merge-tree` reports it.
// Used by: src/merge/merge_tree/engine.rs, src/merge/types.rs
// Used by command/helper: `merge_trees_in_memory` and the `merge_tree` output
#[derive(Debug, Clone)]
pub struct MergeMessage {
    /// Paths the message is about.
    pub paths: Vec<String>,
    /// Machine-readable type for `-z` output (e.g. `Auto-merging`, `CONFLICT (contents)`).
    pub kind: String,
    pub text: String,
}
