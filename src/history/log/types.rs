use crate::diff::rename::types::RenameOptions;
use crate::history::rev_list::types::RevListOptions;

/// How `log` and `show` lay out each commit: git's built-in pretty formats, or a
/// `--format` / `--pretty=format:` template (`terminator` is true for `tformat:`, which ends
/// every entry with a newline, and false for `format:`, which only separates entries).
// Used by: src/history/log/types.rs, src/diff/diff_tree.rs, src/history/log.rs, src/history/log/engine.rs
// Used by command/helper: `parse_log_args`, `format_log_entry`, and the `log`/`show` commands
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LogFormat {
    Oneline,
    Short,
    #[default]
    Medium,
    Full,
    Fuller,
    Raw,
    Template { template: String, terminator: bool },
}

/// How `log --date=<mode>` prints author and committer dates.
// Used by: src/history/log/types.rs, src/history/log/engine.rs
// Used by command/helper: `parse_log_args` and `format_log_date`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogDateMode {
    #[default]
    Default,
    Iso,
    IsoStrict,
    Short,
    Raw,
    Unix,
    Rfc,
    Relative,
}

/// How `log --decorate` names the refs pointing at a commit: not at all, by short name
/// (`main`, `origin/main`, `tag: v1`), or by full name (`refs/heads/main`).
// Used by: src/history/log/types.rs, src/history/log/engine.rs
// Used by command/helper: `parse_log_args`, `collect_decorations`, and the `log`/`show` commands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogDecorate {
    #[default]
    No,
    Short,
    Full,
}

/// Options for `log` and `show`: which commits (the revision walk shared with `rev-list`),
/// how each one is printed, and what diff output follows it.
// Used by: src/diff/diff_tree.rs, src/history/follow.rs, src/history/log.rs, src/history/log/engine.rs, src/merge/merge.rs
// Used by command/helper: `parse_log_args`, `format_log_entry`, and the `log`/`show` commands
#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    pub revs: RevListOptions,
    /// Rename/copy detection for the diff output (`diff.renames`, `-M`, `-C`, `--no-renames`).
    pub renames: RenameOptions,
    /// `--summary`: creations, deletions, renames, copies and mode changes.
    pub summary: bool,
    pub format: LogFormat,
    pub date: LogDateMode,
    pub decorate: LogDecorate,
    /// `-p` / `-u` / `--patch`: the diff against the first parent (none for merges).
    pub patch: bool,
    pub stat: bool,
    pub shortstat: bool,
    pub name_only: bool,
    pub name_status: bool,
    /// Show each commit's note in a `Notes:` block: on by default, off once `--pretty`,
    /// `--format` or `--oneline` is given (`--notes` / `--no-notes` decide explicitly).
    pub show_notes: bool,
    /// `--raw`: one `:<old mode> <new mode> <old hash> <new hash> <status>\t<path>` line per changed path (what `whatchanged` shows).
    pub raw: bool,
    /// Revisions named on the command line, in order (`show` displays each one).
    pub objects: Vec<String>,
    /// `--graph`: draw the commit graph to the left of the output (implies `--topo-order`).
    pub graph: bool,
    /// Whether the output is colored (`--color`, `color.diff`, `color.ui`).
    pub color: bool,
    /// `--follow`: follow one file's history across renames and copies.
    pub follow: bool,
}
