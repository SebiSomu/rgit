//! Command-line interface of `rgit merge-file`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit merge-file`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/merge_file/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_merge_file`
#[derive(clap::Args, Debug)]
pub struct MergeFileCliArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit merge-file` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_merge_file(args: MergeFileCliArgs) -> anyhow::Result<()> {
    let MergeFileCliArgs { args } = args;
    crate::merge::merge_file::merge_file(crate::cli::raw_args_after("merge-file", args))?;
    Ok(())
}
