//! Command-line interface of `rgit sync`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit sync`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/sync/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_sync`
#[derive(clap::Args, Debug)]
pub struct SyncArgs {
    /// Also push the result to the upstream (publishing the branch if it has none yet).
    #[arg(short = 'p', long = "push")]
    push: bool,
    /// Rebase local commits onto the upstream (the default, unless pull.rebase=false).
    #[arg(long = "rebase", conflicts_with = "merge")]
    rebase: bool,
    /// Merge the upstream into the branch instead of rebasing.
    #[arg(long = "merge")]
    merge: bool,
    /// Fetch and print the plan, change nothing else.
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
    /// Skip the fetch: plan against the remote-tracking branch as it is.
    #[arg(long = "no-fetch")]
    no_fetch: bool,
}

/// Runs `rgit sync` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_sync(args: SyncArgs) -> anyhow::Result<()> {
    use crate::remote::sync::types::Integration;
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    let mode = if args.merge {
        Some(Integration::Merge)
    } else if args.rebase {
        Some(Integration::Rebase)
    } else {
        None
    };
    crate::remote::sync::sync(mode, args.push, args.dry_run, !args.no_fetch)
}
