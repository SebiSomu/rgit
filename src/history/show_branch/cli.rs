//! Command-line interface of `rgit show-branch`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit show-branch`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/show_branch/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_show_branch`
#[derive(clap::Args, Debug)]
pub struct ShowBranchArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit show-branch` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_show_branch(args: ShowBranchArgs) -> anyhow::Result<()> {
    let ShowBranchArgs { args } = args;
    crate::history::show_branch::show_branch(crate::cli::raw_args_after("show-branch", args))?;
    Ok(())
}
