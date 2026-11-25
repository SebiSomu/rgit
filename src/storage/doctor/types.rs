//! Types of `rgit doctor`.

use std::path::PathBuf;

/// How bad a finding is: an error means something is broken (exit code 1), a warning means
/// something will likely go wrong or surprise you.
// Used by: src/storage/doctor/types.rs, src/storage/doctor/engine.rs, src/storage/doctor.rs
// Used by command/helper: every `check_*` helper and the `doctor` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A fix `rgit doctor --fix` can apply by itself: removing a stale lock file, pruning
/// worktree records whose directory is gone, or making a hook executable.
// Used by: src/storage/doctor/types.rs, src/storage/doctor/engine.rs, src/storage/doctor.rs
// Used by command/helper: `check_locks`, `check_worktrees`, `check_hooks`, `apply` and the `doctor` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoFix {
    RemoveLock(PathBuf),
    PruneWorktrees,
    MakeExecutable(PathBuf),
}

/// One problem `rgit doctor` found: how bad, what, the command that fixes it (if there is one)
/// and whether `--fix` can apply it.
// Used by: src/storage/doctor/types.rs, src/storage/doctor/engine.rs, src/storage/doctor.rs
// Used by command/helper: every `check_*` helper and the `doctor` command
#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
    pub fix: Option<String>,
    pub auto: Option<AutoFix>,
}

impl Finding {
    /// A finding with a fix to run by hand (or none).
    pub fn new(severity: Severity, message: String, fix: Option<String>) -> Finding {
        Finding { severity, message, fix, auto: None }
    }
}
