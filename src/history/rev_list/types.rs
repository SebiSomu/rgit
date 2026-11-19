use crate::core::types::ParsedCommit;

/// Parsed arguments for the `rev-list` command: which commits to start from and exclude,
/// how to walk and order them, which to keep, and how to print them. Parsed by hand (not by
/// clap) because git's syntax is order-sensitive (`--not`) and allows `-<n>` / `-n<n>`.
// Used by: src/history/cherry/engine.rs, src/history/last_modified/types.rs, src/history/log/types.rs, src/history/rev_list.rs, src/history/rev_list/engine.rs, src/mail/request_pull/engine.rs, src/merge/fmt_merge_msg/engine.rs
// Used by command/helper: `parse_rev_list_args`, `run_rev_list_walk`, `format_rev_list_entry`, and the `rev_list` command
#[derive(Debug, Clone, Default)]
pub struct RevListOptions {
    /// Revision arguments as given (`A`, `^A`, `A..B`, `A...B`); `--not` is already applied as `^`.
    pub revs: Vec<String>,
    /// `--all`, `--branches`, `--tags`, `--remotes`: start from every ref of that kind.
    pub all: bool,
    pub branches: bool,
    pub tags: bool,
    pub remotes: bool,
    /// `-n` / `-<n>` / `--max-count`: show at most this many commits.
    pub max_count: Option<usize>,
    /// `--skip`: drop this many commits before showing any.
    pub skip: usize,
    /// `--reverse`: print the selected commits oldest first.
    pub reverse: bool,
    /// `--count`: print only how many commits would be shown.
    pub count: bool,
    /// `--parents`: print each commit's (possibly rewritten) parents after it.
    pub parents: bool,
    /// `--timestamp`: print the committer timestamp before each commit.
    pub timestamp: bool,
    /// `--left-right`: mark commits `<` (left side of `A...B`) or `>`.
    pub left_right: bool,
    /// `--first-parent`: follow only the first parent of merges.
    pub first_parent: bool,
    /// `--topo-order`: children before parents, keeping lines of history together.
    pub topo_order: bool,
    /// `--date-order`: children before parents, otherwise by committer date.
    pub date_order: bool,
    /// `--abbrev-commit`: print abbreviated (unique, at least 7 character) hashes.
    pub abbrev_commit: bool,
    /// `--min-parents` / `--max-parents` (`--merges` = min 2, `--no-merges` = max 1).
    pub min_parents: Option<usize>,
    pub max_parents: Option<usize>,
    /// `--since` / `--after` and `--until` / `--before`, as Unix timestamps.
    pub since: Option<i64>,
    pub until: Option<i64>,
    /// `--author`, `--committer`, `--grep` regexes, with `--all-match`, `--invert-grep`,
    /// `-i` / `--regexp-ignore-case`, and `-F` / `--fixed-strings`.
    pub authors: Vec<String>,
    pub committers: Vec<String>,
    pub greps: Vec<String>,
    pub all_match: bool,
    pub invert_grep: bool,
    pub ignore_case: bool,
    pub fixed_strings: bool,
    /// `--oneline`, `--pretty=oneline`, `--format=<fmt>`, `--pretty=format:<fmt>` / `tformat:<fmt>`.
    pub format: Option<RevListFormat>,
    /// Paths after `--` (or non-revision arguments naming existing files) that limit history.
    pub paths: Vec<String>,
}

/// How `rev-list` prints each commit beyond its hash line.
// Used by: src/history/rev_list/types.rs, src/history/rev_list.rs, src/history/rev_list/engine.rs
// Used by command/helper: `parse_rev_list_args`, `format_rev_list_entry`, and the `rev_list` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevListFormat {
    /// `<hash> <subject>` on one line.
    Oneline,
    /// A `commit <hash>` line, then the expanded template. `terminator` records `tformat:` vs
    /// `format:`; like git, `rev-list` ends every entry with a newline either way.
    Template { template: String, terminator: bool },
}

/// Where a `rev-list` walk starts: the revisions in the order git adds them (each with whether
/// it is negative, i.e. excluded, and whether it is the left side of a symmetric `A...B`), and
/// the "bottoms" (the excluded commits that were named explicitly, which git still treats as
/// relevant parents when simplifying path-limited history).
// Used by: src/history/cherry/engine.rs, src/history/rev_list/engine.rs, src/mail/request_pull/engine.rs, src/merge/fmt_merge_msg/engine.rs
// Used by command/helper: `resolve_rev_list_tips`, `run_rev_list_walk`, and the `rev_list` command
#[derive(Debug, Clone, Default)]
pub struct RevListRoots {
    pub pending: Vec<(String, bool, bool)>,
    pub bottoms: std::collections::HashSet<String>,
}

/// The mutable state of a `rev-list` walk, mirroring the object flags git keeps per commit:
/// parsed commits (`commits` holds exactly the commits git would have parsed, since an
/// unparsed commit has no known tree or parents) and their current (possibly simplified) parents, `SEEN` (queued once),
/// `ADDED` (parents processed), `UNINTERESTING` (excluded, marked gradually as the walk goes,
/// which is what makes path simplification depend on walk order exactly like git),
/// `TREESAME` (hidden by simplification), and `SYMMETRIC_LEFT`.
// Used by: src/history/rev_list/types.rs, src/history/rev_list/engine.rs
// Used by command/helper: `run_rev_list_walk`, `rewrite_rev_list_parents`, and the `rev_list` command
#[derive(Debug, Clone, Default)]
pub struct RevWalkState {
    pub commits: std::collections::HashMap<String, ParsedCommit>,
    pub parents: std::collections::HashMap<String, Vec<String>>,
    pub seen: std::collections::HashSet<String>,
    pub added: std::collections::HashSet<String>,
    pub uninteresting: std::collections::HashSet<String>,
    pub bottoms: std::collections::HashSet<String>,
    pub treesame: std::collections::HashSet<String>,
    pub left: std::collections::HashSet<String>,
}

impl RevWalkState {
    /// git's `relevant_commit`: not uninteresting, or uninteresting but named explicitly (a bottom).
    pub fn is_relevant(&self, hash: &str) -> bool {
        !self.uninteresting.contains(hash) || self.bottoms.contains(hash)
    }
}
