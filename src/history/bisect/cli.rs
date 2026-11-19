//! Command-line interface of `rgit bisect`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use crate::history::bisect::types::BisectAction;

// Command-line arguments of `rgit bisect`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/bisect/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_bisect`
#[derive(clap::Args, Debug)]
pub struct BisectArgs {
    #[command(subcommand)]
    action: BisectAction
}

/// Runs `rgit bisect` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_bisect(args: BisectArgs) -> anyhow::Result<()> {
    let BisectArgs { action } = args;
    crate::history::bisect::bisect(action)?;
    Ok(())
}
