//! Command-line interface of `rgit conflicts`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit conflicts`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/conflicts/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_conflicts`
#[derive(clap::Args, Debug)]
pub struct ConflictsArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit conflicts` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_conflicts(args: ConflictsArgs) -> anyhow::Result<()> {
    let ConflictsArgs { args } = args;
    crate::merge::conflicts::conflicts(crate::cli::raw_args_after("conflicts", args))?;
    Ok(())
}
