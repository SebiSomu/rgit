/// Represents an individual recorded entry in the stash reference log (`.git/logs/refs/stash`).
// Used by: src/workspace/state_files.rs
// Used by command/helper: `read_stash_list`, `write_stash_list`, and the `stash` command
pub struct StashEntry {
    pub hash: String,
    pub message: String,
}
