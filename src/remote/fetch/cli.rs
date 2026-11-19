//! Command-line interface of `rgit fetch`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit fetch`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/fetch/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_fetch`
#[derive(clap::Args, Debug)]
pub struct FetchArgs {
    remote: Option<String>,
    branch: Option<String>,
}

/// Runs `rgit fetch` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_fetch(args: FetchArgs) -> anyhow::Result<()> {
    let FetchArgs { remote, branch } = args;
    crate::remote::fetch::fetch(remote, branch)?;
    Ok(())
}
