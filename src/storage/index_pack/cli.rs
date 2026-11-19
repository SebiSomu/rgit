//! Command-line interface of `rgit index-pack`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit index-pack`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/index_pack/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_index_pack`
#[derive(clap::Args, Debug)]
pub struct IndexPackArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit index-pack` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_index_pack(args: IndexPackArgs) -> anyhow::Result<()> {
    let IndexPackArgs { args } = args;
    crate::storage::index_pack::index_pack(crate::cli::raw_args_after("index-pack", args))?;
    Ok(())
}
