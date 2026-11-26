//! Command-line interface of `rgit recover`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit recover`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/recover/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_recover`
#[derive(clap::Args, Debug)]
pub struct RecoverArgs {
    /// The number `rgit recover` listed (or the commit) to bring back
    pick: Option<String>,
    /// Name of the branch to create (default: recovered-<commit>)
    branch: Option<String>,
}

/// Runs `rgit recover` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_recover(args: RecoverArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    match args.pick {
        None => crate::workspace::recover::list(),
        Some(pick) => crate::workspace::recover::restore(&pick, args.branch.as_deref()),
    }
}
