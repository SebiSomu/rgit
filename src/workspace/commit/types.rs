/// Options for the `commit` command: where the message comes from (`-m`, `-F`, `-C`/`-c`, the
/// editor), what to commit (`-a`, `--amend`, `--allow-empty`), and who and when it is
/// attributed to (`--author`, `--date`, `--reset-author`, `-s`).
// Used by: src/cli.rs, src/merge/cherry_pick.rs, src/workspace/commit.rs
// Used by command/helper: the `commit` command
#[derive(Debug, Clone, Default)]
pub struct CommitOptions {
    /// `-m` values; several are joined as separate paragraphs.
    pub messages: Vec<String>,
    /// `-F <file>` (`-` reads standard input).
    pub file: Option<String>,
    /// `-C <commit>` (reuse message and author) or `-c <commit>` (same, then edit).
    pub reuse_message: Option<String>,
    pub reedit: bool,
    /// `-a`: stage modified and deleted tracked files first.
    pub all: bool,
    /// `--amend`: replace the tip commit instead of adding one on top.
    pub amend: bool,
    /// `-e` forces the editor, `--no-edit` suppresses it.
    pub edit: bool,
    pub no_edit: bool,
    pub author: Option<String>,
    pub date: Option<String>,
    /// `--reset-author`: with `--amend` or `-C`, make the committer the author again.
    pub reset_author: bool,
    pub allow_empty: bool,
    pub allow_empty_message: bool,
    /// `-s`: add a `Signed-off-by:` trailer for the committer.
    pub signoff: bool,
    /// `--cleanup=strip`: drop `#` lines even without the editor.
    pub cleanup_strip: bool,
    pub quiet: bool,
}
