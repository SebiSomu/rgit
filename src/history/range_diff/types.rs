/// Parsed arguments for the `range-diff` command, which compares two versions of a commit range.
// Used by: src/history/range_diff/engine.rs
// Used by command/helper: `parse_range_diff_args`, `format_range_diff`, and the `range_diff` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RangeDiffOptions {
    /// The old and the new range (`<base>..<tip>`).
    pub range1: String,
    pub range2: String,
    /// `--creation-factor=<n>`: how readily two commits are treated as unrelated, in percent (default 60).
    pub creation_factor: i64,
    /// `--left-only` / `--right-only`: leave out commits that exist only in the other range.
    pub left_only: bool,
    pub right_only: bool,
    /// Off with `-s` / `--no-patch`: don't show the patch comparison of changed pairs.
    pub patch: bool,
}

/// One commit of a range as `range-diff` sees it: its position, hash and subject, its
/// normalized patch text (metadata, message, then the diff starting at `diff_offset`), the
/// diff's size in lines, the commit of the other range it is paired with, and whether it
/// has been printed.
// Used by: src/history/range_diff/engine.rs
// Used by command/helper: `read_range_patches`, `match_range_patches`, `format_range_diff`, and the `range_diff` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RangePatch {
    pub index: usize,
    pub hash: String,
    pub subject: String,
    pub patch: String,
    pub diff_offset: usize,
    pub diffsize: usize,
    pub matching: Option<usize>,
    pub shown: bool,
}
