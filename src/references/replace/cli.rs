//! Command-line interface of `rgit replace`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit replace`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/replace/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_replace`
#[derive(clap::Args, Debug)]
pub struct ReplaceArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit replace` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_replace(args: ReplaceArgs) -> anyhow::Result<()> {
    let ReplaceArgs { args } = args;
    crate::references::replace::replace(crate::cli::raw_args_after("replace", args))?;
    Ok(())
}
