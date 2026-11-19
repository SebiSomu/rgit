/// How serious a problem found by `fsck` is: an error means the repository is damaged and
/// makes `fsck` exit non-zero; a warning is worth knowing about but doesn't.
// Used by: src/storage/fsck/types.rs, src/storage/fsck.rs
// Used by command/helper: `FsckIssue`, `FsckReport`, and the `fsck` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsckSeverity {
    Error,
    Warning,
}

/// One problem found by `fsck`, with a human-readable description.
// Used by: src/storage/fsck/types.rs, src/storage/fsck/engine.rs
// Used by command/helper: `verify_loose_object`, `check_refs`, `check_index`, `mark_reachable_in_graph`, and the `fsck` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsckIssue {
    pub severity: FsckSeverity,
    pub message: String,
}

impl FsckIssue {
    /// A problem that means the repository is damaged.
    pub fn error(message: String) -> FsckIssue {
        FsckIssue { severity: FsckSeverity::Error, message }
    }
 
    /// A problem worth reporting that doesn't mean damage.
    pub fn warning(message: String) -> FsckIssue {
        FsckIssue { severity: FsckSeverity::Warning, message }
    }
}

/// One object in the graph `fsck` builds: its type (`blob`, `tree`, `commit`, `tag`, or
/// `unknown` if it could not be read) and the objects it links to, each with the type the link
/// claims they have (`any` for a tag's target).
// Used by: src/storage/fsck/engine.rs
// Used by command/helper: `verify_loose_object`, `mark_reachable_in_graph`, and `run_fsck_checks`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsckNode {
    pub kind: String,
    pub links: Vec<(String, &'static str)>,
}

/// Everything an `fsck` run found: how many objects it looked at, the problems, and the objects
/// nothing reaches (all of them, and the subset that nothing else unreachable points at, which
/// are called dangling). Types and hashes are stored as `(type, hash)` pairs.
// Used by: src/storage/fsck/types.rs, src/storage/fsck/engine.rs
// Used by command/helper: `run_fsck_checks` and the `fsck` command
#[derive(Debug, Clone, Default)]
pub struct FsckReport {
    pub objects_checked: usize,
    pub issues: Vec<FsckIssue>,
    pub dangling: Vec<(String, String)>,
    pub unreachable: Vec<(String, String)>,
}

impl FsckReport {
    /// Number of problems that mean damage.
    pub fn error_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == FsckSeverity::Error).count()
    }
 
    /// Number of problems that don't.
    pub fn warning_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity == FsckSeverity::Warning).count()
    }
}
