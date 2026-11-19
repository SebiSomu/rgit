/// Parsed arguments for the `fmt-merge-msg` command, which writes the default message of a
/// merge commit.
// Used by: src/merge/fmt_merge_msg/engine.rs
// Used by command/helper: `parse_fmt_merge_msg_args`, `format_merge_message`, and the `fmt_merge_msg` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FmtMergeMsgOptions {
    /// `--log[=<n>]` / `merge.log`: list up to this many commit subjects per merged tip; 0 turns the shortlog off.
    pub shortlog_len: usize,
    /// `-m <text>`: use this text instead of the generated `Merge ...` title.
    pub message: Option<String>,
    /// `--into-name <name>`: pretend the merge goes into this branch.
    pub into_name: Option<String>,
    /// `-F <file>`: read the `FETCH_HEAD`-style lines from this file instead of standard input.
    pub file: Option<String>,
}

/// What a merge takes from one repository (the text after ` of ` on `FETCH_HEAD` lines,
/// `.` for the local one), grouped by kind for the title. `head_status` has bit 1 when the
/// repository's HEAD is merged and bit 2 when named refs are.
// Used by: src/merge/fmt_merge_msg/engine.rs
// Used by command/helper: `format_merge_message` and its title helpers
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeSource {
    pub name: String,
    pub branches: Vec<String>,
    pub tags: Vec<String>,
    pub remote_branches: Vec<String>,
    pub generic: Vec<String>,
    pub head_status: u8,
}

/// One tip being merged: its display name for the shortlog (`topic`, or `'topic' of
/// <repo>`), the object named on its `FETCH_HEAD` line, and whether it is a branch of the
/// local repository (only those can have a branch description).
// Used by: src/merge/fmt_merge_msg/engine.rs
// Used by command/helper: `format_merge_message` and its tag-message / shortlog helpers
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MergeOrigin {
    pub name: String,
    pub hash: String,
    pub is_local_branch: bool,
}
