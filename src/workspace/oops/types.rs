/// One way out of a likely mistake, as `rgit oops` suggests it: a one-line title, a sentence on
/// why rgit thinks this happened and what the fix does, the exact rgit commands that fix it
/// (each as its argument list, without `rgit`), and a score used to rank the suggestions
/// (higher = more likely what the user wants).
// Used by: src/workspace/oops/types.rs, src/workspace/oops/engine.rs, src/workspace/oops.rs
// Used by command/helper: every `suggest_*` detector, `collect_fixes` and the `oops` command
#[derive(Debug, Clone)]
pub struct Fix {
    pub title: String,
    pub why: String,
    pub commands: Vec<Vec<String>>,
    pub score: u32,
}

/// What the detectors look at, read once: HEAD, the current branch and its tip, the branch's
/// upstream tip, every branch / tag / remote-tracking tip, HEAD's reflog (newest first), the
/// operation log, whether the working tree or index has uncommitted changes, the operation in
/// progress and the current time.
// Used by: src/workspace/oops/types.rs, src/workspace/oops/engine.rs
// Used by command/helper: `gather_context` and every `suggest_*` detector
#[derive(Debug, Clone)]
pub struct OopsContext {
    /// Current branch name; None when HEAD is detached.
    pub branch: Option<String>,
    pub head: Option<String>,
    /// Tip of the current branch's upstream (`branch.<b>.remote` / `.merge`), if any.
    pub upstream: Option<(String, String)>,
    /// All ref tips (local branches, tags, remote-tracking refs), name -> commit.
    pub ref_tips: std::collections::BTreeMap<String, String>,
    pub head_reflog: Vec<crate::core::types::ReflogEntry>,
    pub ops: Vec<crate::workspace::undo::types::Operation>,
    pub dirty: bool,
    pub in_progress: Option<String>,
    pub now: i64,
}
