/// Filter mode determining whether untracked files, ignored files, or both are targeted for cleaning.
// Used by: src/workspace/clean/types.rs, src/workspace/clean.rs, src/workspace/clean/engine.rs, src/workspace/state_files.rs
// Used by command/helper: `collect_clean_candidates` helper and the `clean` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanMode {
    Normal,
    IncludeIgnored,
    IgnoredOnly,
}

impl CleanMode {
    pub fn wants(&self, ignored: bool) -> bool {
        match self {
            CleanMode::Normal => !ignored,
            CleanMode::IncludeIgnored => true,
            CleanMode::IgnoredOnly => ignored,
        }
    }
}
