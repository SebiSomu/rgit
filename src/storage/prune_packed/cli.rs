//! Command-line interface of `rgit prune-packed`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit prune-packed`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/prune_packed/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_prune_packed`
#[derive(clap::Args, Debug)]
pub struct PrunePackedArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit prune-packed` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_prune_packed(args: PrunePackedArgs) -> anyhow::Result<()> {
    let PrunePackedArgs { args } = args;
    crate::storage::prune_packed::prune_packed(crate::cli::raw_args_after("prune-packed", args))?;
    Ok(())
}
