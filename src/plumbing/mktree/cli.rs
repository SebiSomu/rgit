//! Command-line interface of `rgit mktree`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit mktree`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/mktree/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_mktree`
#[derive(clap::Args, Debug)]
pub struct MktreeArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit mktree` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_mktree(args: MktreeArgs) -> anyhow::Result<()> {
    let MktreeArgs { args } = args;
    crate::plumbing::mktree::mktree(crate::cli::raw_args_after("mktree", args))?;
    Ok(())
}
