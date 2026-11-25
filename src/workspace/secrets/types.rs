//! Types of `rgit secrets`.

/// What `rgit secrets` looks at: the staged changes (the default, for a pre-commit hook), every
/// tracked file as staged, or the lines each commit added (`--history`).
// Used by: src/workspace/secrets/types.rs, src/workspace/secrets/cli.rs, src/workspace/secrets.rs
// Used by command/helper: `dispatch_secrets` and the `secrets` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Staged,
    AllFiles,
    History(Vec<String>),
}

/// One possible secret: where (commit for `--history`, path, line; no line for a file whose
/// name says it holds secrets), what kind, and the matched text, masked.
// Used by: src/workspace/secrets/types.rs, src/workspace/secrets/engine.rs, src/workspace/secrets.rs
// Used by command/helper: `scan_text`, `scan_name` and the `secrets` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub commit: Option<String>,
    pub path: String,
    pub line: Option<usize>,
    pub what: String,
    pub masked: String,
}
