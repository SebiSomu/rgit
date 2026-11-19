//! Command-line interface of `rgit clone`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit clone`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/clone/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_clone`
#[derive(clap::Args, Debug)]
pub struct CloneArgs {
    repository: String,
    directory: Option<String>,
    #[arg(short = 'b', long = "branch")]
    branch: Option<String>,
    #[arg(short = 'o', long = "origin")]
    origin: Option<String>,
}

/// Runs `rgit clone` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_clone(args: CloneArgs) -> anyhow::Result<()> {
    let CloneArgs { repository, directory, branch, origin } = args;
    crate::remote::clone::clone(repository, directory, branch, origin)?;
    Ok(())
}
