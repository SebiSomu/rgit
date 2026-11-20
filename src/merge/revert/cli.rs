//! Command-line interface of `rgit revert`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit revert`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/revert/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_revert`
#[derive(clap::Args, Debug)]
pub struct RevertArgs {
    commit: Option<String>,
    #[arg(short = 'n', long = "no-commit", conflicts_with_all = ["cont", "abort"])]
    no_commit: bool,
    #[arg(long = "continue", conflicts_with_all = ["abort", "no_commit"])]
    cont: bool,
    #[arg(long = "abort", conflicts_with_all = ["cont", "no_commit"])]
    abort: bool,
    // Accepted for git compatibility: rgit never opens an editor for the revert message.
    #[arg(long = "no-edit")]
    no_edit: bool,
}

/// Runs `rgit revert` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_revert(args: RevertArgs) -> anyhow::Result<()> {
    let RevertArgs { commit, no_commit, cont, abort, no_edit: _ } = args;
    crate::merge::revert::revert(commit, no_commit, cont, abort)?;
    Ok(())
}
