//! Command-line interface of `rgit submodule`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit submodule`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/submodule/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_submodule`
#[derive(clap::Args, Debug)]
pub struct SubmoduleArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit submodule` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_submodule(args: SubmoduleArgs) -> anyhow::Result<()> {
    let SubmoduleArgs { args } = args;
    crate::workspace::submodule::submodule(crate::cli::raw_args_after("submodule", args))?;
    Ok(())
}
