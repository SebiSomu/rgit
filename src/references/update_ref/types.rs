/// Parsed arguments for the `update-ref` command. Parsed by hand like git's option parser
/// (bundled switches such as `-dz`, `-m<reason>` or `-m <reason>`, options after the names,
/// `--` ending option parsing), so usage errors can exit 129 as git does.
// Used by: src/references/update_ref.rs, src/references/update_ref/engine.rs
// Used by command/helper: `parse_update_ref_args` and the `update_ref` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateRefOptions {
    /// `-m <reason>`: reflog message for every update (empty message when absent).
    pub message: Option<String>,
    /// `-d`: delete `<ref>` instead of updating it.
    pub delete: bool,
    /// `--no-deref`: update `<ref>` itself even when it is a symbolic ref (`--deref` undoes it).
    pub no_deref: bool,
    /// `--stdin`: read a transaction of commands from stdin instead of the arguments.
    pub stdin: bool,
    /// `-z`: stdin commands and their arguments are NUL-terminated.
    pub nul_terminated: bool,
    /// `--create-reflog`: create a reflog for the updated refs even outside heads/remotes/HEAD.
    pub create_reflog: bool,
    /// `<ref> <new-oid> [<old-oid>]`, or `<ref> [<old-oid>]` with `-d`.
    pub args: Vec<String>,
}

/// One queued change in a ref transaction (`update-ref` arguments or an `--stdin` command).
/// Hashes are full hex; the all-zero hash in `new_hash` means "delete", in `old_hash` "must not
/// exist yet". `new_hash: None` is a `verify` (check only), `old_hash: None` skips the check.
// Used by: src/references/update_ref.rs, src/references/update_ref/engine.rs
// Used by command/helper: `queue_ref_update`, `prepare_ref_transaction`, and the `update_ref` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefUpdate {
    pub refname: String,
    pub new_hash: Option<String>,
    pub old_hash: Option<String>,
    /// Update `refname` itself rather than the ref it points at when it is symbolic.
    pub no_deref: bool,
    /// Force a reflog for this ref (`--create-reflog`).
    pub create_reflog: bool,
}

/// A [`RefUpdate`] whose checks passed: the ref file that will actually change (after
/// following symbolic refs unless `no_deref`) and the value it holds right now.
// Used by: src/references/update_ref/engine.rs
// Used by command/helper: `prepare_ref_transaction` and `commit_ref_transaction`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRefUpdate {
    /// The ref file written or deleted (`refs/heads/main` for an update of `HEAD`).
    pub target: String,
    /// What `target` resolves to before the transaction (`None` if it doesn't exist).
    pub current: Option<String>,
    pub new_hash: Option<String>,
    pub create_reflog: bool,
    /// `target` is the branch HEAD is on, so the change is logged in HEAD's reflog too, even
    /// when the value stays the same.
    pub logs_head: bool,
}

/// Where an `update-ref --stdin` session stands, in git's order: `Open` (implicit
/// transaction, committed at end of input), `Started` (after `start`), `Prepared` (after
/// `prepare`; only `commit`/`abort` may follow) and `Closed` (after `commit`/`abort`; only
/// `start` may follow).
// Used by: src/references/update_ref/engine.rs
// Used by command/helper: `run_update_ref_stdin` (for `update-ref --stdin`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpdateRefsState {
    Open,
    Started,
    Prepared,
    Closed,
}
