//! Command-line interface of `rgit repo`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit repo`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/repo_info/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_repo`
#[derive(clap::Args, Debug)]
pub struct RepoArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit repo` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_repo(args: RepoArgs) -> anyhow::Result<()> {
    let RepoArgs { args } = args;
    crate::plumbing::repo_info::repo(crate::cli::raw_args_after("repo", args))?;
    Ok(())
}
