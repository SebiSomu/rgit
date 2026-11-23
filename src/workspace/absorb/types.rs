//! Types of `rgit absorb`.

/// One staged hunk of a file, against HEAD's version, without context lines: the HEAD lines
/// it replaces (`old_start`, 0-based, and `old_len`) and the lines that replace them.
// Used by: src/workspace/absorb/types.rs, src/workspace/absorb/engine.rs, src/workspace/absorb.rs
// Used by command/helper: `hunks_of`, `place`, `apply_hunks` and the `absorb` command
#[derive(Debug, Clone)]
pub struct StagedHunk {
    pub path: String,
    pub old_start: usize,
    pub old_len: usize,
    pub new_lines: Vec<String>,
}

/// Where `rgit absorb` puts a hunk: into a commit (its hash), or nowhere (why it stays staged).
// Used by: src/workspace/absorb/types.rs, src/workspace/absorb/engine.rs, src/workspace/absorb.rs
// Used by command/helper: `place` and the `absorb` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    Into(String),
    Left(String),
}
