//! Command-line interface of `rgit range-diff`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit range-diff`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/range_diff/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_range_diff`
#[derive(clap::Args, Debug)]
pub struct RangeDiffArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit range-diff` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_range_diff(args: RangeDiffArgs) -> anyhow::Result<()> {
    let RangeDiffArgs { args } = args;
    crate::history::range_diff::range_diff(crate::cli::raw_args_after("range-diff", args))?;
    Ok(())
}
