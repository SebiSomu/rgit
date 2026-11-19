/// Represents the result or update status of a reference during a push operation.
// Used by: src/remote/github.rs, src/remote/push.rs
// Used by command/helper: `push` command and push reporting helpers
#[derive(Debug, PartialEq, Eq)]
pub enum PushStatus {
    // Never constructed before the refactor either (hidden while in the `pub` objects.rs).
    #[allow(dead_code)]
    UpToDate,
    FastForward { old_hash: String, new_hash: String },
    ForcedUpdate { old_hash: String, new_hash: String },
    NewBranch { new_hash: String },
    #[allow(dead_code)]
    Deleted,
}
