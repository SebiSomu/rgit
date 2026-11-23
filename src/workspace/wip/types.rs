//! Types of `rgit wip`.

/// One WIP save of a branch, as `refs/rgit/wip/<branch>` keeps it: the `w` commit (its tree is
/// the working tree), the commit it was saved on, its index commit, its untracked-files commit
/// (an empty tree when there were none), the save below it on the branch's stack (None for the
/// oldest), the message (first line) and when it was saved (seconds since the epoch).
// Used by: src/workspace/wip/types.rs, src/workspace/wip/engine.rs, src/workspace/wip.rs
// Used by command/helper: `read_stack`, `remove_entry` and every `rgit wip` action
#[derive(Debug, Clone)]
pub struct WipEntry {
    pub commit: String,
    pub base: String,
    pub index: String,
    pub untracked: String,
    pub below: Option<String>,
    pub message: String,
    pub time: i64,
}
