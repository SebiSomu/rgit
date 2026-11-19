use crate::diff::rename::types::RenameOptions;

/// Parsed arguments of the `fast-export` command.
// Used by: src/export/fast_export/engine.rs
// Used by command/helper: `parse_fast_export_args`, `run_fast_export`, and the `fast_export` command
#[derive(Debug, Clone, Default)]
pub struct FastExportOptions {
    /// `--signed-tags=<mode>`: `abort` (default), `verbatim`, `warn`, `warn-strip` or `strip`.
    pub signed_tags: String,
    /// `--tag-of-filtered-object=<mode>`: `abort` (default) or `drop`.
    pub tag_of_filtered: String,
    /// `--export-marks=<file>`.
    pub export_marks: Option<String>,
    /// `--import-marks=<file>` (the flag is true for `--import-marks-if-exists`).
    pub import_marks: Option<(String, bool)>,
    /// `--no-data`: refer to blobs by name instead of exporting them.
    pub no_data: bool,
    /// `--full-tree`: every commit lists all its files after a `deleteall`.
    pub full_tree: bool,
    /// `--use-done-feature`: frame the stream with `feature done` / `done`.
    pub use_done: bool,
    /// `--mark-tags`: give tag objects marks too.
    pub mark_tags: bool,
    /// `--reference-excluded-parents`: name excluded parents by hash instead of dropping them.
    pub reference_excluded: bool,
    /// `--show-original-ids`: add `original-oid` lines.
    pub show_original_ids: bool,
    /// The revision arguments.
    /// `-M` / `-C`: report renames and copies (`R` / `C` commands).
    pub renames: RenameOptions,
    pub revs: Vec<String>,
}
