//! Command-line interface of `rgit get-tar-commit-id`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit get-tar-commit-id`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/export/get_tar_commit_id/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_get_tar_commit_id`
#[derive(clap::Args, Debug)]
pub struct GetTarCommitIdArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit get-tar-commit-id` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_get_tar_commit_id(args: GetTarCommitIdArgs) -> anyhow::Result<()> {
    let GetTarCommitIdArgs { args } = args;
    crate::export::get_tar_commit_id::get_tar_commit_id(crate::cli::raw_args_after("get-tar-commit-id", args))?;
    Ok(())
}
