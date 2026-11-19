/// Parsed arguments of `rgit cherry-pick`; also what `.git/sequencer/opts` stores.
// Used by: src/cli.rs, src/merge/cherry_pick.rs, src/merge/sequencer.rs
// Used by command/helper: the `cherry-pick` command and `start_sequence` / `read_sequence_options`
#[derive(Debug, Clone, Default)]
pub struct CherryPickOptions {
    /// Commits and ranges (`A..B`, `^X`) to pick.
    pub commits: Vec<String>,
    /// `-n`: apply to the index and working tree only.
    pub no_commit: bool,
    /// `-x`: append `(cherry picked from commit <hash>)`.
    pub record_origin: bool,
    /// `-s`: append `Signed-off-by:` for the committer.
    pub signoff: bool,
    /// `-m <n>`: diff a merge against its n-th parent.
    pub mainline: Option<usize>,
    /// `--ff`: fast-forward when `HEAD` is the commit's parent.
    pub ff: bool,
    pub allow_empty: bool,
    pub allow_empty_message: bool,
    pub keep_redundant: bool,
    pub cont: bool,
    pub skip: bool,
    pub abort: bool,
    pub quit: bool,
}
