/// Parsed arguments of the `name-rev` command.
// Used by: src/history/name_rev/engine.rs
// Used by command/helper: `parse_name_rev_args`, `build_rev_names`, and the `name_rev` command
#[derive(Debug, Clone, Default)]
pub struct NameRevOptions {
    /// `--name-only`: print only the name, not the argument it belongs to.
    pub name_only: bool,
    /// `--tags`: only use tags to name commits.
    pub tags: bool,
    /// `--refs=<pattern>` (repeatable): only use refs matching one of the patterns.
    pub refs: Vec<String>,
    /// `--exclude=<pattern>` (repeatable): never use refs matching one of the patterns.
    pub exclude: Vec<String>,
    /// `--all`: list every commit reachable from the refs with its name.
    pub all: bool,
    /// `--annotate-stdin` (or the older `--stdin`): copy standard input, adding names after hashes.
    pub annotate_stdin: bool,
    /// `--no-undefined`: fail instead of printing `undefined` for a revision without a name.
    pub no_undefined: bool,
    /// `--always`: fall back to the abbreviated hash for a revision without a name.
    pub always: bool,
    /// The revisions to name.
    pub revs: Vec<String>,
}

/// The best name `name-rev` has found for a commit so far: the ref it is reached from and
/// how — `generation` first-parent steps after the last merge hop recorded in `tip_name`.
/// `distance` (first-parent steps count 1, other parents 65535) decides between names.
// Used by: src/history/name_rev/types.rs, src/history/name_rev/engine.rs
// Used by command/helper: `build_rev_names`, `rev_name_text`, and the `name_rev` command
#[derive(Debug, Clone)]
pub struct RevName {
    /// Ref-relative prefix such as `main`, `tags/v1^0` or `main~3^2`.
    pub tip_name: String,
    /// Tagger date of the tag the name starts at (commit date for other refs); older wins ties.
    pub taggerdate: i64,
    /// First-parent steps from `tip_name` to this commit (printed as `~N`).
    pub generation: u32,
    /// Total weighted distance from the ref.
    pub distance: u32,
    /// Whether the name starts at a ref under `refs/tags/` (preferred over other refs).
    pub from_tag: bool,
}

/// Everything `name-rev` worked out from the refs: the best name of every commit it reached,
/// and the name of each object a ref points at directly (for tags, trees and blobs).
// Used by: src/history/name_rev/engine.rs
// Used by command/helper: `build_rev_names`, `object_name`, `annotate_line`, and the `name_rev` command
#[derive(Debug, Clone, Default)]
pub struct RevNames {
    /// Commit hash → best name.
    pub commits: std::collections::HashMap<String, RevName>,
    /// Object a ref points at → that ref's short name.
    pub tips: std::collections::HashMap<String, String>,
}

/// A ref `name-rev` may name commits after: its short name, the commit it leads to (if any),
/// the date used to rank it, and whether it is a tag / an annotated tag that was peeled.
// Used by: src/history/name_rev/engine.rs
// Used by command/helper: `build_rev_names` and the `name_rev` command
#[derive(Debug, Clone)]
pub struct NameRevTip {
    /// Short ref name, e.g. `main` or `tags/v1`.
    pub name: String,
    /// The commit the ref points at, directly or through annotated tags.
    pub commit: Option<String>,
    /// Tagger date of an annotated tag, else the commit's date.
    pub taggerdate: i64,
    /// Whether the ref is under `refs/tags/` and leads to a commit.
    pub from_tag: bool,
    /// Whether an annotated tag was peeled to reach the commit (names it `<tag>^0`).
    pub deref: bool,
}
