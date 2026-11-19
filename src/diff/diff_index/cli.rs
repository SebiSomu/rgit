//! Command-line interface of `rgit diff-index`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit diff-index`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/diff/diff_index/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_diff_index`
#[derive(clap::Args, Debug)]
pub struct DiffIndexArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit diff-index` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_diff_index(args: DiffIndexArgs) -> anyhow::Result<()> {
    let DiffIndexArgs { args } = args;
    crate::diff::diff_index::diff_index(crate::cli::raw_args_after("diff-index", args))?;
    Ok(())
}
