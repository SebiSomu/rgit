//! Command-line interface of `rgit rev-list`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit rev-list`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/rev_list/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_rev_list`
#[derive(clap::Args, Debug)]
pub struct RevListArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit rev-list` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_rev_list(args: RevListArgs) -> anyhow::Result<()> {
    let RevListArgs { args } = args;
    crate::history::rev_list::rev_list(crate::cli::raw_args_after("rev-list", args))?;
    Ok(())
}
