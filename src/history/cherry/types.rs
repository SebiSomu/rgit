/// Options for the `cherry` command, which marks the commits of `<head>` missing from
/// `<upstream>` as `+` (not applied there) or `-` (an equivalent patch is already upstream).
// Used by: src/cli.rs, src/history/cherry.rs
// Used by command/helper: the `cherry` command
#[derive(Debug, Clone, Default)]
pub struct CherryOptions {
    /// `-v` / `--verbose`: print each commit's subject after its hash.
    pub verbose: bool,
    /// `--abbrev[=<n>]`: shorten hashes to at least `n` characters (7 without a value); full hashes when `None`.
    pub abbrev: Option<usize>,
    /// The branch to compare against; the current branch's upstream when absent.
    pub upstream: Option<String>,
    /// The commits to check; `HEAD` when absent.
    pub head: Option<String>,
    /// Leave out commits reachable from this one.
    pub limit: Option<String>,
}
