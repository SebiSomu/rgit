//! Command-line interface of `rgit column`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit column`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/column/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_column`
#[derive(clap::Args, Debug)]
pub struct ColumnArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit column` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_column(args: ColumnArgs) -> anyhow::Result<()> {
    let ColumnArgs { args } = args;
    crate::plumbing::column::column(crate::cli::raw_args_after("column", args))?;
    Ok(())
}
