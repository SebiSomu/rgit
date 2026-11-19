/// Everything `fast-import` keeps while reading a stream.
// Used by: src/export/fast_import.rs, src/export/fast_import/engine.rs
// Used by command/helper: `run_fast_import`, `finish_fast_import`, and the `fast_import` command
#[derive(Debug, Clone, Default)]
pub struct FastImportState {
    /// Mark number → object name.
    pub marks: std::collections::BTreeMap<u64, String>,
    /// Ref name → its current tip in the stream (`None` after a `reset` without `from`).
    pub refs: std::collections::BTreeMap<String, Option<String>>,
    /// Ref name → the flattened tree of its tip: path → (hash, mode).
    pub trees: std::collections::HashMap<String, std::collections::BTreeMap<String, ([u8; 20], u32)>>,
    /// Refs to delete at the end (`reset <ref>` from the all-zero hash).
    pub deleted: Vec<String>,
    /// How dates are given: `raw` (default), `raw-permissive` or `now`.
    pub date_format: String,
    /// `--force` / `feature force`: update refs even when history would be lost.
    pub force: bool,
    /// `--done` / `feature done`: the stream must end with a `done` command.
    pub require_done: bool,
    /// Whether a `done` command was read.
    pub saw_done: bool,
    /// `--export-marks=<file>` / `feature export-marks=<file>`.
    pub export_marks: Option<String>,
    /// Counts of objects created, for `--stats`: blobs, trees, commits, tags.
    pub counts: [usize; 4],
}

/// A cursor over the bytes of a `fast-import` stream.
// Used by: src/export/fast_import/engine.rs
// Used by command/helper: `run_fast_import`, `import_commit`, and the `fast_import` command
#[derive(Debug)]
pub struct FastImportInput<'a> {
    /// The whole stream.
    pub data: &'a [u8],
    /// Position of the next unread byte.
    pub pos: usize,
}
