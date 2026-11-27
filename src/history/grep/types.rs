/// Options controlling pattern search, target files, and output formatting for `grep`.
// Used by: src/cli.rs, src/history/grep.rs, src/history/grep/engine.rs
// Used by command/helper: `grep` command and `search_content` helper
#[derive(Debug, Clone)]
pub struct GrepOptions {
    pub ignore_case: bool,
    pub invert_match: bool,
    pub line_number: bool,
    pub count: bool,
    pub files_with_matches: bool,
    pub files_without_match: bool,
    pub fixed_strings: bool,
    pub cached: bool,
    /// Color file names, separators, line numbers and matches as git does.
    pub color: bool,
}

/// The result of searching a single file or blob in `grep`.
// Used by: src/history/grep.rs, src/history/grep/engine.rs
// Used by command/helper: `grep` command and `search_content` helper
#[derive(Debug, Clone)]
pub struct GrepFileResult {
    pub display_path: String,
    pub is_binary: bool,
    pub matches: Vec<(usize, String)>,
}
