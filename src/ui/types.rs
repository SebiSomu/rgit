/// How much rgit narrates around a command: `Quiet` prints exactly what git would, `Normal`
/// adds one-line notes about what actually happened, conflict reports and warnings, `Explain`
/// adds what it means, `Teach` adds the refs and index changes underneath.
// Used by: src/cli.rs, src/ui.rs
// Used by command/helper: `ui::init`, `ui::level` and every narration helper in ui.rs
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Verbosity {
    Quiet,
    Normal,
    Explain,
    Teach,
}

/// Repository state captured before and after a command so rgit can narrate the difference:
/// HEAD, all refs, the index (stage 0 and unmerged stage masks), what is staged against HEAD,
/// the stash count, the operation in progress and (when needed) the working tree's tracked
/// changes and untracked files.
// Used by: src/ui.rs, src/ui/narrate.rs
// Used by command/helper: `take_snapshot` and the exit-time narration
#[derive(Debug, Clone, Default)]
pub struct RepoSnapshot {
    /// Branch HEAD points at; None when detached.
    pub head_branch: Option<String>,
    pub head: Option<String>,
    pub refs: std::collections::BTreeMap<String, String>,
    pub index: std::collections::BTreeMap<String, [u8; 20]>,
    /// Unmerged paths and which of stages 1-3 each has (bit `stage - 1`).
    pub unmerged: std::collections::BTreeMap<String, u8>,
    /// Paths whose staged content differs from HEAD (new, modified or deleted).
    pub staged: Vec<String>,
    pub stash_count: usize,
    pub operation: Option<String>,
    pub worktree_changes: Option<Vec<(char, String)>>,
    pub untracked: Option<Vec<String>>,
}
