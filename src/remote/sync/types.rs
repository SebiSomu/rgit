//! Types of `rgit sync`.

/// How `rgit sync` brings in upstream commits when the branch has its own too.
// Used by: src/remote/sync/types.rs, src/remote/sync/engine.rs, src/remote/sync.rs
// Used by command/helper: `integration_mode`, `print_plan` and the `sync` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integration {
    Rebase,
    Merge,
}

/// Where the current branch stands against its upstream, after the fetch: the branch, the
/// remote and the branch's name there, the remote-tracking tip (None when there is no upstream
/// or it is gone) and the commit counts each side has that the other lacks.
// Used by: src/remote/sync/types.rs, src/remote/sync/engine.rs, src/remote/sync.rs
// Used by command/helper: `upstream_of`, `print_plan` and the `sync` command
#[derive(Debug, Clone)]
pub struct SyncState {
    pub branch: String,
    pub remote: String,
    pub remote_branch: String,
    /// Whether `branch.<name>.remote`/`merge` are set.
    pub configured: bool,
    pub tracking_tip: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

impl SyncState {
    /// `origin/main`.
    pub fn tracking_name(&self) -> String {
        format!("{}/{}", self.remote, self.remote_branch)
    }
}
