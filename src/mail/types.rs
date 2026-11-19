/// Planned results of checking a set of file patches, keyed by path: `Some((content, mode))`
/// for a file to write, `None` for a file to delete. Nothing touches disk until every patch
/// in the set has been checked, which is what makes `apply` and `am` all-or-nothing.
// Used by: src/mail/types.rs, src/mail/am.rs, src/mail/apply.rs, src/mail/apply/engine.rs
// Used by command/helper: `check_file_patch`, `load_patch_target`, `patch_target_exists`, `write_planned_changes`, and the `apply`/`am` commands
pub type PlannedChanges = std::collections::BTreeMap<String, Option<(Vec<u8>, u32)>>;
