use crate::workspace::types::GitIgnoreRule;

/// Options for the `check-ignore` command, which reports which paths are excluded by
/// `.gitignore` rules and, in verbose mode, which rule excluded them.
// Used by: src/cli.rs, src/workspace/check_ignore.rs
// Used by command/helper: the `check_ignore` command (validation, input source, and output format)
#[derive(Debug, Clone, Default)]
pub struct CheckIgnoreOptions {
    /// `-q` / `--quiet`: print nothing; only the exit status reports the result (single path only).
    pub quiet: bool,
    /// `-v` / `--verbose`: print `<source>:<line>:<pattern>\t<path>` for the matching rule.
    pub verbose: bool,
    /// `-n` / `--non-matching`: with `-v`, also print paths that matched no rule as `::\t<path>`.
    pub non_matching: bool,
    /// `--no-index`: also check tracked paths, which are skipped by default.
    pub no_index: bool,
    /// `--stdin`: read paths from standard input, one per line.
    pub stdin: bool,
    /// `-z`: with `--stdin`, input and output records are NUL-terminated.
    pub null_terminated: bool,
    /// Paths given on the command line.
    pub paths: Vec<String>,
}

/// A parsed `.gitignore` rule together with where it was defined: the `.gitignore` file,
/// the 1-based line number, and the pattern text as written (including `!` and trailing `/`).
// Used by: src/workspace/check_ignore.rs, src/workspace/check_ignore/engine.rs
// Used by command/helper: `find_ignore_match` helper and the `check_ignore` command (`-v` output)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcedIgnoreRule {
    pub rule: GitIgnoreRule,
    pub source: String,
    pub line_number: usize,
    pub pattern_text: String,
}
