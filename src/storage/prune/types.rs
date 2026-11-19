use std::path::PathBuf;

/// A file in `.git/objects` that is not a loose object: a temporary file left by an
/// interrupted write (`tmp_obj_*` in a fan-out directory, `tmp_*` in `objects/` or
/// `objects/pack/`), with its modification time, or any other stray file in a fan-out directory.
// Used by: src/storage/prune.rs, src/storage/prune/engine.rs
// Used by command/helper: `list_object_dir_cruft` and the `prune` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectDirCruft {
    StaleTemp { path: PathBuf, modified: u64 },
    Bad { path: PathBuf },
}
