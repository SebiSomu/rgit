/// Parsed arguments for the `replay` command, which re-creates commits on a new base
/// without touching the working tree.
// Used by: src/merge/replay/engine.rs
// Used by command/helper: `parse_replay_args`, `replay_commits`, and the `replay` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayOptions {
    /// `--onto <newbase>`: the commit to replay onto.
    pub onto: Option<String>,
    /// `--advance <branch>`: replay on top of this branch and move it.
    pub advance: Option<String>,
    /// `--contained`: move every branch pointing into the range, not just the range tips.
    pub contained: bool,
    /// The revision ranges whose commits are replayed.
    pub ranges: Vec<String>,
}
