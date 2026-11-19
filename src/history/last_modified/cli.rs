//! Command-line interface of `rgit last-modified`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit last-modified`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/last_modified/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_last_modified`
#[derive(clap::Args, Debug)]
pub struct LastModifiedArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit last-modified` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_last_modified(args: LastModifiedArgs) -> anyhow::Result<()> {
    let LastModifiedArgs { args } = args;
    crate::history::last_modified::last_modified(crate::cli::raw_args_after("last-modified", args))?;
    Ok(())
}
