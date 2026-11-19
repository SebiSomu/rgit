/// One block of conflict markers in a file: its 1-based line range, the lines of each side
/// (and of the common ancestor, with the diff3 style) and the labels after the markers.
// Used by: src/merge/conflicts.rs, src/ui/narrate.rs
// Used by command/helper: `parse_conflict_hunks`, the conflict reports and `rgit conflicts`
#[derive(Debug, Clone)]
pub struct ConflictHunk {
    pub start: usize,
    pub end: usize,
    pub ours: Vec<String>,
    pub base: Option<Vec<String>>,
    pub theirs: Vec<String>,
    pub ours_label: String,
    pub theirs_label: String,
}
