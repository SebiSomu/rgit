/// The switches of `update-index` that apply to the paths and actions after them.
// Used by: src/workspace/index_plumbing.rs, src/workspace/update_index.rs
// Used by command/helper: `update_index_path`, `add_cache_entry`, and the `update_index` command
#[derive(Debug, Clone, Default)]
pub struct UpdateIndexFlags {
    /// `--add`: paths not yet in the index may be added.
    pub add: bool,
    /// `--replace`: a file may replace a directory of entries and the other way round.
    pub replace: bool,
    /// `--remove`: a path missing from the working tree is removed from the index.
    pub remove: bool,
    /// `--force-remove`: remove the paths from the index even when the files exist.
    pub force_remove: bool,
    /// `--info-only`: record the hash without writing the blob to the object store.
    pub info_only: bool,
    /// `--verbose`: report `add '<path>'` / `remove '<path>'` / `chmod ±x '<path>'`.
    pub verbose: bool,
    /// `--chmod=+x` (`Some(true)`) or `--chmod=-x` (`Some(false)`).
    pub chmod: Option<bool>,
    /// `-q`: `--refresh` does not report or fail on entries that need an update.
    pub quiet: bool,
    /// `--ignore-missing`: `--refresh` skips entries whose file is missing.
    pub ignore_missing: bool,
    /// `--skip-worktree` (`Some(true)`) / `--no-skip-worktree` (`Some(false)`): only set or
    /// clear the skip-worktree bit of the following paths.
    pub skip_worktree: Option<bool>,
}
