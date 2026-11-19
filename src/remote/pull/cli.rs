//! Command-line interface of `rgit pull`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit pull`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/pull/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_pull`
#[derive(clap::Args, Debug)]
pub struct PullArgs {
    remote: Option<String>,
    branch: Option<String>,
}

/// Runs `rgit pull` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_pull(args: PullArgs) -> anyhow::Result<()> {
    let PullArgs { remote, branch } = args;
    crate::remote::pull::pull(remote, branch)?;
    Ok(())
}
