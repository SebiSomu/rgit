//! Command-line interface of `rgit fast-export`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit fast-export`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/export/fast_export/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_fast_export`
#[derive(clap::Args, Debug)]
pub struct FastExportArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit fast-export` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_fast_export(args: FastExportArgs) -> anyhow::Result<()> {
    let FastExportArgs { args } = args;
    crate::export::fast_export::fast_export(crate::cli::raw_args_after("fast-export", args))?;
    Ok(())
}
