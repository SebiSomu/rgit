/// The parsed command line of `rgit ls-remote`.
// Used by: src/remote/ls_remote.rs, src/remote/ls_remote/engine.rs
// Used by command/helper: `parse_ls_remote_args`, `select_refs` and the `ls-remote` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LsRemoteOptions {
    /// `--heads` / `--branches`: only `refs/heads/`.
    pub heads: bool,
    /// `--tags`: only `refs/tags/` (with `--heads`: both).
    pub tags: bool,
    /// `--refs`: no `HEAD` and no peeled `^{}` entries.
    pub refs_only: bool,
    /// `--symref`: also show where symbolic refs point (`ref: refs/heads/main\tHEAD`).
    pub symref: bool,
    /// `-q` / `--quiet`: no `From <url>` line.
    pub quiet: bool,
    /// `--exit-code`: exit with 2 when nothing matched.
    pub exit_code: bool,
    /// `--get-url`: print the remote's URL and stop.
    pub get_url: bool,
    pub repository: Option<String>,
    pub patterns: Vec<String>,
}

/// One line of the listing before filtering: a ref, or the `^{}` entry that carries the
/// commit an annotated tag points at.
// Used by: src/remote/ls_remote/engine.rs
// Used by command/helper: `select_refs`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedRef {
    pub name: String,
    pub oid: String,
    pub symref_target: Option<String>,
}
