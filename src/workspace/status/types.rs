/// Options of `rgit status`: the short/porcelain format and its `-b` header and `-z`
/// terminators, and which untracked files are shown (`no`, `normal` = collapsed directories,
/// `all` = every file).
// Used by: src/cli.rs, src/merge/cherry_pick.rs, src/workspace/staging.rs, src/workspace/status.rs
// Used by command/helper: the `status` command and `format_short_status`
#[derive(Debug, Clone, Default)]
pub struct StatusOptions {
    pub short: bool,
    pub branch: bool,
    pub nul: bool,
    pub untracked: String,
}
