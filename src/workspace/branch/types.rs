/// Parsed `rgit branch` command line: which mode runs (list / create / delete / rename / copy /
/// upstream changes / show-current), its flags, the already-resolved filter commits, and the
/// remaining non-option arguments (branch names, start point or list patterns).
// Used by: src/workspace/branch.rs, src/workspace/branch/engine.rs
// Used by command/helper: `parse_branch_args`, `format_branch_list` and the `branch` command
#[derive(Debug, Clone, Default)]
pub struct BranchOptions {
    pub delete: bool,
    pub rename: bool,
    pub copy: bool,
    /// `-f`, also set by `-D` / `-M` / `-C`.
    pub force: bool,
    pub remotes: bool,
    pub all: bool,
    pub list: bool,
    /// `-v` count: 1 shows hash + subject, 2 adds the upstream name.
    pub verbose: u8,
    pub quiet: bool,
    /// `--track` (Some(true)) / `--no-track` (Some(false)); None follows git's default.
    pub track: Option<bool>,
    pub set_upstream: Option<String>,
    pub unset_upstream: bool,
    pub show_current: bool,
    /// rgit's `--push[=<remote>]` with `-m`: also rename the branch on the remote (Some(None):
    /// the branch's own remote).
    pub push: Option<Option<String>>,
    pub merged: Vec<String>,
    pub no_merged: Vec<String>,
    pub contains: Vec<String>,
    pub no_contains: Vec<String>,
    pub points_at: Vec<String>,
    pub args: Vec<String>,
}

/// One row of `rgit branch` listing output: display name, short name for sorting/patterns,
/// kind (0 = detached HEAD, 1 = local, 2 = remote-tracking), tip, and markers.
// Used by: src/workspace/branch/engine.rs
// Used by command/helper: `format_branch_list`
#[derive(Debug, Clone)]
pub struct BranchListEntry {
    pub name: String,
    pub short: String,
    pub kind: u8,
    pub oid: String,
    /// The current branch (or the detached HEAD row): printed with `* `.
    pub current: bool,
    /// Path of another worktree that has this branch checked out (`+ `, and `-vv`).
    pub other_worktree: Option<String>,
    /// Target of a symbolic remote-tracking ref (`origin/HEAD -> origin/main`).
    pub symref: Option<String>,
}

/// What `branch -m <old> <new> --push` does on the remote: which remote, the name the branch
/// has there now (its upstream's name, usually `<old>`), and whether there is one to delete.
// Used by: src/workspace/branch.rs, src/workspace/branch/types.rs
// Used by command/helper: `plan_remote_rename`, `rename_on_remote`
#[derive(Debug, Clone)]
pub struct RemoteRename {
    pub remote: String,
    pub old_remote_branch: String,
    pub delete_old: bool,
}
