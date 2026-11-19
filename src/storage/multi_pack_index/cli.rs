//! Command-line interface of `rgit multi-pack-index`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit multi-pack-index`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/multi_pack_index/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_multi_pack_index`
#[derive(clap::Args, Debug)]
pub struct MultiPackIndexArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit multi-pack-index` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_multi_pack_index(args: MultiPackIndexArgs) -> anyhow::Result<()> {
    let MultiPackIndexArgs { args } = args;
    crate::storage::multi_pack_index::multi_pack_index(crate::cli::raw_args_after("multi-pack-index", args))?;
    Ok(())
}
