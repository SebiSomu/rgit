//! Command-line interface of `rgit commit-graph`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit commit-graph`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/commit_graph/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_commit_graph`
#[derive(clap::Args, Debug)]
pub struct CommitGraphArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit commit-graph` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_commit_graph(args: CommitGraphArgs) -> anyhow::Result<()> {
    let CommitGraphArgs { args } = args;
    crate::storage::commit_graph::commit_graph(crate::cli::raw_args_after("commit-graph", args))?;
    Ok(())
}
