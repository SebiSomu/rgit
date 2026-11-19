/// The answer given to one hunk in patch mode (`add -p` and friends).
// Used by: src/workspace/patch/types.rs, src/workspace/patch.rs, src/workspace/patch/engine.rs
// Used by command/helper: `run_patch_mode`, `patch_update_file`, `apply_file_hunks`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HunkUse {
    #[default]
    Undecided,
    Use,
    Skip,
}

/// What patch mode changes when the chosen hunks are applied: the index, the working tree,
/// both (`checkout -p <tree-ish>`), or a scratch tree that becomes a stash (`stash -p`).
// Used by: src/workspace/patch/types.rs, src/workspace/patch.rs, src/workspace/patch/engine.rs
// Used by command/helper: `PatchModeSpec`, `apply_patch_file`, and the `-p` commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchTarget {
    Index,
    Worktree,
    Both,
    Stash,
}

/// One of git's patch modes (`add-patch.c`'s `struct patch_mode`): the four prompts (mode
/// change, deletion, addition, hunk; `%s` takes the extra commands), the hint shown when editing
/// a hunk, the mode-specific help lines, whether the diff is applied in reverse, and the target.
// Used by: src/workspace/patch.rs, src/workspace/patch/engine.rs
// Used by command/helper: `run_patch_mode` and the `add`/`reset`/`checkout`/`restore`/`stash` `-p` commands
#[derive(Debug, Clone, Copy)]
pub struct PatchModeSpec {
    pub prompts: [&'static str; 4],
    pub edit_hint: &'static str,
    pub help: &'static str,
    pub is_reverse: bool,
    pub target: PatchTarget,
}

/// One hunk of a patch-mode diff: its `@@` numbers, the text after the second `@@` (function
/// context with its newline, or empty), its lines (each with marker and line end), the answer,
/// how many change groups it could be split into, and whether it is the pseudo hunk standing for
/// a file's `old mode`/`new mode` lines.
// Used by: src/workspace/patch/types.rs, src/workspace/patch/engine.rs
// Used by command/helper: `parse_patch_file`, `split_hunk`, `edit_hunk`, `apply_file_hunks`
#[derive(Debug, Clone, Default)]
pub struct InteractiveHunk {
    pub old_offset: usize,
    pub old_count: usize,
    pub new_offset: usize,
    pub new_count: usize,
    pub extra: Vec<u8>,
    pub lines: Vec<Vec<u8>>,
    pub use_: HunkUse,
    pub splittable_into: usize,
    pub mode_change: bool,
    /// git's hunk for a deleted file: it starts at the `deleted file mode` line, so the rest of
    /// the header and the original `@@` line are part of its text (no `@@` line is generated).
    pub raw: bool,
}

/// One file in patch mode: its diff header (without the mode lines, which become the first
/// pseudo hunk), its hunks, what kind of change it is, and both sides of the diff (entry and
/// content), so the chosen hunks can be applied to whichever side is the target.
// Used by: src/workspace/patch.rs, src/workspace/patch/engine.rs
// Used by command/helper: `parse_patch_file`, `patch_update_file`, `apply_patch_file`
#[derive(Debug, Clone, Default)]
pub struct InteractiveFile {
    pub path: String,
    pub header: Vec<Vec<u8>>,
    pub hunks: Vec<InteractiveHunk>,
    pub mode_change: bool,
    pub deleted: bool,
    pub added: bool,
    pub binary: bool,
    /// The answer for a file without hunks (an empty added file), git's `file_diff->head`.
    pub head_use: HunkUse,
    pub old: Option<([u8; 20], u32)>,
    pub new: Option<([u8; 20], u32)>,
    pub old_bytes: Vec<u8>,
    pub new_bytes: Vec<u8>,
}
