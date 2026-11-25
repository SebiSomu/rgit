//! Command-line interface of `rgit move-commit`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit move-commit`, parsed by clap through `cli::Commands` (a `///`
// doc comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/move_commit/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_move_commit`
#[derive(clap::Args, Debug)]
pub struct MoveCommitArgs {
    /// The commits to move: hashes, revisions or ranges like HEAD~3..
    #[arg(required = true, value_name = "COMMIT")]
    commits: Vec<String>,
    /// The branch to move them to.
    #[arg(value_name = "BRANCH")]
    branch: String,
    /// Create the branch if it doesn't exist, at the parent of the oldest commit moved.
    #[arg(short = 'c', long = "create")]
    create: bool,
    /// Copy the commits to the branch, keep them on the current one.
    #[arg(long = "copy")]
    copy: bool,
    /// Also drop commits a remote-tracking branch already has.
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit move-commit` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_move_commit(args: MoveCommitArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::merge::move_commit::move_commit(args.commits, args.branch, args.create, args.copy, args.force)
}
