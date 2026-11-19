//! Command-line interface of `rgit worktree`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use crate::workspace::worktree::types::WorktreeAction;

// Command-line arguments of `rgit worktree`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/worktree/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_worktree`
#[derive(clap::Args, Debug)]
pub struct WorktreeArgs {
    #[command(subcommand)]
    action: WorktreeAction,
}

/// Runs `rgit worktree` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_worktree(args: WorktreeArgs) -> anyhow::Result<()> {
    let WorktreeArgs { action } = args;
    crate::workspace::worktree::worktree(action)?;
    Ok(())
}
