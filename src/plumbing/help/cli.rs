//! Command-line interface of `rgit help`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit help`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/help/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_help`
#[derive(clap::Args, Debug)]
pub struct HelpArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit help` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_help(args: HelpArgs) -> anyhow::Result<()> {
    let HelpArgs { args } = args;
    crate::plumbing::help::help(crate::cli::raw_args_after("help", args))?;
    Ok(())
}
