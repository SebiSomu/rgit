//! Types of `rgit ignore`.

/// One pattern given to `rgit ignore`, after checking it against the ignore file: its text,
/// whether the file already had it, and the tracked and untracked paths it matches.
// Used by: src/workspace/ignore/types.rs, src/workspace/ignore/engine.rs, src/workspace/ignore.rs
// Used by command/helper: `plan_pattern` and the `ignore` command
#[derive(Debug, Clone)]
pub struct PatternPlan {
    pub pattern: String,
    pub already: bool,
    pub tracked: Vec<String>,
    pub untracked: usize,
}
