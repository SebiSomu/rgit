//! Types of `rgit fixup`.

/// One commit `rgit fixup` offers in its picker: hash, subject and the remote-tracking branch that already has it (`origin/main`), if any.
// Used by: src/workspace/fixup/types.rs, src/workspace/fixup/engine.rs, src/workspace/fixup.rs
// Used by command/helper: `candidates`, `pick_interactively` and the `fixup` command
#[derive(Debug, Clone)]
pub struct Candidate {
    pub hash: String,
    pub subject: String,
    pub pushed_on: Option<String>,
}
