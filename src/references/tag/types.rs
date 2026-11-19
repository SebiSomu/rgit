/// Options for the `tag` command: list (with patterns, `-n` annotation lines, `--contains`,
/// `--points-at`), create (lightweight, or annotated with `-a`/`-m`/`-F`, replacing with `-f`),
/// or delete tags.
// Used by: src/cli.rs, src/references/tag.rs
// Used by command/helper: the `tag` command
#[derive(Debug, Clone, Default)]
pub struct TagOptions {
    pub list: bool,
    pub delete: bool,
    pub annotate: bool,
    pub force: bool,
    pub messages: Vec<String>,
    pub file: Option<String>,
    pub edit: bool,
    /// `-n[<num>]`: show this many lines of each tag's message (1 when given without a number).
    pub lines: Option<usize>,
    pub contains: Option<String>,
    pub points_at: Option<String>,
    /// Positional arguments: `<tagname> [<object>]` when creating, names when deleting,
    /// patterns when listing.
    pub args: Vec<String>,
}

/// Options for the `describe` command, which names a commit after the nearest tag.
// Used by: src/cli.rs, src/references/tag.rs
// Used by command/helper: `describe_commit` and the `describe` command
#[derive(Debug, Clone, Default)]
pub struct DescribeOptions {
    /// `--tags`: lightweight tags count too (otherwise only annotated ones).
    pub tags: bool,
    /// `--always`: fall back to the abbreviated hash when no tag is found.
    pub always: bool,
    /// `--long`: always print `<tag>-<n>-g<hash>`, even for an exact match.
    pub long: bool,
    /// `--abbrev=<n>`: hash length (0 prints just the tag).
    pub abbrev: Option<usize>,
    /// `--exact-match`: fail unless a tag points straight at the commit.
    pub exact_match: bool,
    pub commits: Vec<String>,
}
