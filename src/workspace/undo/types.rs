use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Everything `rgit undo` can put back, as recorded before and after an operation: where HEAD
/// points, every local ref, the index, the stash list, the in-progress operation files
/// (MERGE_HEAD, rebase-merge/, ...) and, for commands that touch files, a working-tree snapshot.
/// Saved data lives in the log's private store (`.git/rgit/objects`) and is referenced here by id.
// Used by: src/workspace/undo/types.rs, src/workspace/undo/engine.rs, src/workspace/undo.rs
// Used by command/helper: `capture_state`, `plan_restore`, `apply_restore` and the `undo` / `redo` commands
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoState {
    /// Contents of `.git/HEAD`, trimmed: `ref: refs/heads/main` or a commit id.
    pub head: String,
    /// Every direct (non-symbolic) ref except remote-tracking ones, name -> object id.
    pub refs: BTreeMap<String, String>,
    /// Blob id of the raw `.git/index` file (None when there is no index).
    pub index: Option<String>,
    /// Fingerprint of the index's entries (stage, mode, object, path), ignoring stat data,
    /// so a command that only refreshes timestamps does not count as a change.
    pub index_key: Option<String>,
    /// Blob id of `.git/STASH_LIST` (None when the stash is empty).
    pub stash: Option<String>,
    /// Manifest id of the in-progress operation files of `.git` (None when there are none).
    pub state_files: Option<String>,
    /// Manifest id of the working tree (tracked + untracked, not ignored), in clean (index) form.
    /// Only recorded for commands that can rewrite files.
    pub worktree: Option<String>,
}

/// What kind of entry an operation-log line is: a regular command, or an `undo` / `redo` of
/// an earlier entry (identified by its id).
// Used by: src/workspace/undo/types.rs, src/workspace/undo/engine.rs, src/workspace/undo.rs
// Used by command/helper: `undo_stacks`, `append_operation` and the `undo` / `redo` commands
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OpKind {
    Command,
    Undo { target: u64 },
    Redo { target: u64 },
}

/// One recorded state-changing rgit invocation: its id, when it ran (Unix seconds), the
/// command line as typed, and the repository state before and after it. Stored one JSON object
/// per line in `.git/rgit/oplog`.
// Used by: src/workspace/undo/types.rs, src/workspace/undo/engine.rs, src/workspace/undo.rs
// Used by command/helper: `read_oplog`, `append_operation`, `finish_recording` and the `undo` / `redo` commands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    pub id: u64,
    pub time: i64,
    pub command: String,
    pub kind: OpKind,
    pub before: RepoState,
    pub after: RepoState,
}

/// A command's "before" state, held in memory while the command runs. The raw index bytes are
/// kept so the index blob is only written when the command actually changed the index.
// Used by: src/workspace/undo/types.rs, src/workspace/undo/engine.rs
// Used by command/helper: `begin_recording`, `finish_recording`, `capture_state`
#[derive(Debug, Clone)]
pub struct Captured {
    pub state: RepoState,
    pub index_bytes: Option<Vec<u8>>,
}

/// One thing `undo` / `redo` will change, described for the preview and used to apply it.
// Used by: src/workspace/undo/types.rs, src/workspace/undo/engine.rs, src/workspace/undo.rs
// Used by command/helper: `plan_restore`, `apply_restore`, `describe_change` and the `undo` / `redo` commands
#[derive(Debug, Clone)]
pub enum Change {
    /// `.git/HEAD` goes from one value to another.
    Head { from: String, to: String },
    /// A ref is created (`from` None), deleted (`to` None) or moved.
    Ref { name: String, from: Option<String>, to: Option<String> },
    /// The index is put back (None = removed).
    Index { to: Option<String> },
    /// The stash list is put back (None = emptied); entry counts for the preview.
    Stash { to: Option<String>, from_count: usize, to_count: usize },
    /// The in-progress operation files are put back; names before and after.
    StateFiles { to: Option<String>, from_names: Vec<String>, to_names: Vec<String> },
    /// Working-tree files are rewritten (`write`, with their clean blob id and mode) or removed.
    Files { write: Vec<(String, [u8; 20], u32)>, remove: Vec<String> },
}

/// The result of checking an undo / redo against the repository as it is now: the changes to
/// make and, when something the operation touched has changed again since, why it can't be
/// applied cleanly.
// Used by: src/workspace/undo/types.rs, src/workspace/undo/engine.rs, src/workspace/undo.rs
// Used by command/helper: `plan_restore` and the `undo` / `redo` commands
#[derive(Debug, Clone, Default)]
pub struct RestorePlan {
    pub changes: Vec<Change>,
    pub conflicts: Vec<String>,
}
