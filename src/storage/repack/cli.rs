//! Command-line interface of `rgit repack`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit repack`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/repack/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_repack`
#[derive(clap::Args, Debug)]
pub struct RepackArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit repack` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_repack(args: RepackArgs) -> anyhow::Result<()> {
    let RepackArgs { args } = args;
    crate::storage::repack::repack(crate::cli::raw_args_after("repack", args))?;
    Ok(())
}
