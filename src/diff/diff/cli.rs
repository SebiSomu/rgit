//! Command-line interface of `rgit diff`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit diff`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/diff/diff/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_diff`
#[derive(clap::Args, Debug)]
pub struct DiffArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit diff` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_diff(args: DiffArgs) -> anyhow::Result<()> {
    let DiffArgs { args } = args;
    crate::diff::diff::diff(crate::cli::raw_args_after("diff", args))?;
    Ok(())
}
