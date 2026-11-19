//! Command-line interface of `rgit update-index`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit update-index`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/update_index/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_update_index`
#[derive(clap::Args, Debug)]
pub struct UpdateIndexArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit update-index` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_update_index(args: UpdateIndexArgs) -> anyhow::Result<()> {
    let UpdateIndexArgs { args } = args;
    crate::workspace::update_index::update_index(crate::cli::raw_args_after("update-index", args))?;
    Ok(())
}
