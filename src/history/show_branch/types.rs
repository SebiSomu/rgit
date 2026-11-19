/// Parsed arguments for the `show-branch` command, which shows branches and the commits
/// that set them apart.
// Used by: src/history/show_branch/engine.rs
// Used by command/helper: `parse_show_branch_args`, `collect_show_branch_names`, `show_branch_output`, and the `show_branch` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShowBranchOptions {
    /// `-a`: all branches and remote-tracking branches; `-r`: remote-tracking branches.
    pub all_heads: bool,
    pub all_remotes: bool,
    /// `--more=<n>`: commits to show beyond the common ancestor; -1 (`--list`) shows only the headers.
    pub extra: i64,
    /// `--no-name` / `--sha1-name`: omit the `[name]` column, or show abbreviated hashes in it.
    pub no_name: bool,
    pub sha1_name: bool,
    /// `--current`: add the current branch to the tips.
    pub current: bool,
    /// `--merge-base` / `--independent`: print merge bases, or the tips no other tip contains.
    pub merge_base: bool,
    pub independent: bool,
    /// `--date-order` instead of the default topological order.
    pub date_order: bool,
    /// `--topics`: only commits that are not on the first tip.
    pub topics: bool,
    /// `--sparse`: don't hide merges reachable from a single tip.
    pub sparse: bool,
    /// Revisions and ref globs named on the command line.
    pub revs: Vec<String>,
}
