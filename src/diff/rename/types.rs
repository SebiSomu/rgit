/// Which renames/copies `diffcore_rename` looks for (git's `-M`, `-C`, `--find-copies-harder`,
/// `-l`, `diff.renames`): the kind, whether unmodified files can be copy sources, the minimum
/// similarity (out of 60000) and the rename limit.
// Used by: src/diff/rename/types.rs, src/diff/plumbing/types.rs, src/diff/rename.rs, src/export/fast_export/types.rs, src/history/log/types.rs
// Used by command/helper: `detect_renames`, `parse_rename_option`, and the diff/log/status/fast-export commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenameOptions {
    /// 0 = off, 1 = renames, 2 = renames and copies.
    pub level: u8,
    pub find_copies_harder: bool,
    pub min_score: u32,
    pub limit: usize,
}

impl Default for RenameOptions {
    fn default() -> Self {
        RenameOptions { level: 0, find_copies_harder: false, min_score: 30000, limit: 1000 }
    }
}
