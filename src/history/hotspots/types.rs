//! Types of `rgit hotspots`.

use std::collections::BTreeMap;

/// One file's change history over the commits looked at: how many commits changed it, the
/// lines they added and removed, and how many commits each author made to it.
// Used by: src/history/hotspots/types.rs, src/history/hotspots/engine.rs, src/history/hotspots.rs
// Used by command/helper: `collect` and the `hotspots` command
#[derive(Debug, Clone, Default)]
pub struct FileStats {
    pub commits: usize,
    pub added: usize,
    pub removed: usize,
    pub authors: BTreeMap<String, usize>,
}

/// What `rgit hotspots` found: the number of commits looked at, per-file stats, how many
/// commits changed each pair of files together (`(a, b)` with `a < b`), and how many commits
/// were too big to count for that.
// Used by: src/history/hotspots/types.rs, src/history/hotspots/engine.rs, src/history/hotspots.rs
// Used by command/helper: `collect` and the `hotspots` command
#[derive(Debug, Clone, Default)]
pub struct Hotspots {
    pub commits: usize,
    pub files: BTreeMap<String, FileStats>,
    pub pairs: BTreeMap<(String, String), usize>,
    pub bulk: usize,
}
