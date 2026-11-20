/// Represents the result or update status of a reference during a push operation.
// Used by: src/remote/github.rs, src/remote/push.rs
// Used by command/helper: `push` command and push reporting helpers
#[derive(Debug, PartialEq, Eq)]
pub enum PushStatus {
    FastForward { old_hash: String, new_hash: String },
    ForcedUpdate { old_hash: String, new_hash: String },
    NewBranch { new_hash: String },
}
