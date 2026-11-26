//! Command-line interface of `rgit drop`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit drop`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/drop/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_drop`
#[derive(clap::Args, Debug)]
pub struct DropArgs {
    /// The commit to remove from the current branch
    commit: String,
    /// Drop it even if it is already pushed
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit drop` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_drop(args: DropArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::workspace::drop::drop(&args.commit, args.force)
}
