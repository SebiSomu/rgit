//! Command-line interface of `rgit squash`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit squash`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/squash/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_squash`
#[derive(clap::Args, Debug)]
pub struct SquashArgs {
    /// How many commits to fold together, counting HEAD (2 or more)
    count: usize,
    /// Use this message instead of opening the editor
    #[arg(short = 'm', long = "message", conflicts_with = "no_edit")]
    message: Option<String>,
    /// Keep the commits' messages, joined, without opening the editor
    #[arg(long = "no-edit")]
    no_edit: bool,
    /// Squash even if some of the commits are already pushed
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit squash` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_squash(args: SquashArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::workspace::squash::squash(args.count, args.message, args.no_edit, args.force)
}
