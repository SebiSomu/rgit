/// Options for the `shortlog` command, which summarises `git log` output grouped by author.
// Used by: src/cli.rs, src/history/shortlog.rs, src/history/shortlog/engine.rs
// Used by command/helper: `collect_shortlog` and the `shortlog` command
#[derive(Debug, Clone, Default)]
pub struct ShortlogOptions {
    /// `-s` / `--summary`: show only commit counts, not subject lines.
    pub summary: bool,
    /// `-n` / `--numbered`: sort output by number of commits (descending) instead of alphabetically.
    pub numbered: bool,
    /// `-e` / `--email`: include the email address in the author key / display.
    pub email: bool,
    /// `--all`: walk every ref in `refs/heads/` and `refs/tags/` instead of just HEAD.
    pub all: bool,
    /// Starting revision (default: HEAD when empty).
    pub rev: Option<String>,
}
