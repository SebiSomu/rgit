//! Command-line interface of `rgit diff-files`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit diff-files`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/diff/diff_files/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_diff_files`
#[derive(clap::Args, Debug)]
pub struct DiffFilesArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit diff-files` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_diff_files(args: DiffFilesArgs) -> anyhow::Result<()> {
    let DiffFilesArgs { args } = args;
    crate::diff::diff_files::diff_files(crate::cli::raw_args_after("diff-files", args))?;
    Ok(())
}
