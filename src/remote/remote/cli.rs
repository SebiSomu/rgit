//! Command-line interface of `rgit remote`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use crate::remote::remote::types::RemoteAction;

// Command-line arguments of `rgit remote`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/remote/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_remote`
#[derive(clap::Args, Debug)]
pub struct RemoteArgs {
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
    #[command(subcommand)]
    action: Option<RemoteAction>,
}

/// Runs `rgit remote` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_remote(args: RemoteArgs) -> anyhow::Result<()> {
    let RemoteArgs { verbose, action } = args;
    crate::remote::remote::remote(verbose, action)?;
    Ok(())
}
