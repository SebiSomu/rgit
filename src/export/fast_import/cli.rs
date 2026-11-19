//! Command-line interface of `rgit fast-import`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit fast-import`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/export/fast_import/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_fast_import`
#[derive(clap::Args, Debug)]
pub struct FastImportArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit fast-import` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_fast_import(args: FastImportArgs) -> anyhow::Result<()> {
    let FastImportArgs { args } = args;
    crate::export::fast_import::fast_import(crate::cli::raw_args_after("fast-import", args))?;
    Ok(())
}
