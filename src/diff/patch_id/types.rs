/// Parsed arguments for the `patch-id` command: which of git's patch-ID algorithms to use.
// Used by: src/diff/patch_id/engine.rs
// Used by command/helper: `parse_patch_id_args`, `compute_patch_ids`, and the `patch_id` command
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PatchIdOptions {
    /// `--stable` (or `patchid.stable`): hash each file's diff separately and sum the hashes,
    /// so the ID doesn't depend on file order. Off means git's default "unstable" ID.
    pub stable: bool,
    /// `--verbatim` (or `patchid.verbatim`): don't strip whitespace before hashing; implies `stable`.
    pub verbatim: bool,
}
