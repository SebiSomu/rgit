//! Command-line interface of `rgit mktag`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit mktag`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/mktag/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_mktag`
#[derive(clap::Args, Debug)]
pub struct MktagArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit mktag` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_mktag(args: MktagArgs) -> anyhow::Result<()> {
    let MktagArgs { args } = args;
    crate::plumbing::mktag::mktag(crate::cli::raw_args_after("mktag", args))?;
    Ok(())
}
