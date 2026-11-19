//! Command-line interface of `rgit show-index`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit show-index`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/show_index/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_show_index`
#[derive(clap::Args, Debug)]
pub struct ShowIndexArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit show-index` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_show_index(args: ShowIndexArgs) -> anyhow::Result<()> {
    let ShowIndexArgs { args } = args;
    crate::storage::show_index::show_index(crate::cli::raw_args_after("show-index", args))?;
    Ok(())
}
