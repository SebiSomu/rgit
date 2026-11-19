//! Command-line interface of `rgit stash`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use crate::workspace::stash::types::StashAction;

// Command-line arguments of `rgit stash`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/stash/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_stash`
#[derive(clap::Args, Debug)]
pub struct StashArgs {
    #[arg(short = 'p', long = "patch")]
    patch: bool,
    #[arg(short = 'u', long = "include-untracked", conflicts_with = "all")]
    include_untracked: bool,
    #[arg(short = 'a', long = "all")]
    all: bool,
    #[command(subcommand)]
    action: Option<StashAction>
}

/// Runs `rgit stash` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_stash(args: StashArgs) -> anyhow::Result<()> {
    match args {
        StashArgs { patch: true, action: None, .. } => {
            crate::workspace::patch::stash_patch(None, Vec::new())?;
        }
        StashArgs { action: None, include_untracked, all, .. } if include_untracked || all => {
            crate::workspace::stash::stash(Some(crate::workspace::stash::types::StashAction::Push { message: None, patch: false, include_untracked, all, paths: Vec::new() }))?;
        }
        StashArgs { action, .. } => {
            crate::workspace::stash::stash(action)?;
        }
    }
    Ok(())
}
