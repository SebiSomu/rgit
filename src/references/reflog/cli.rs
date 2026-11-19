//! Command-line interface of `rgit reflog`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use crate::references::reflog::types::ReflogAction;

// Command-line arguments of `rgit reflog`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/reflog/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_reflog`
#[derive(clap::Args, Debug)]
pub struct ReflogArgs {
    #[command(subcommand)]
    action: Option<ReflogAction>,
}

/// Runs `rgit reflog` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_reflog(args: ReflogArgs) -> anyhow::Result<()> {
    let ReflogArgs { action } = args;
    crate::references::reflog::reflog(action)?;
    Ok(())
}
