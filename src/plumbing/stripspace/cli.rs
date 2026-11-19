//! Command-line interface of `rgit stripspace`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit stripspace`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/stripspace/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_stripspace`
#[derive(clap::Args, Debug)]
pub struct StripspaceArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit stripspace` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_stripspace(args: StripspaceArgs) -> anyhow::Result<()> {
    let StripspaceArgs { args } = args;
    crate::plumbing::stripspace::stripspace(crate::cli::raw_args_after("stripspace", args))?;
    Ok(())
}
