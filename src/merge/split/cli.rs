//! Command-line interface of `rgit split`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit split`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/split/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_split`
#[derive(clap::Args, Debug)]
pub struct SplitArgs {
    /// The commit to split (default: HEAD).
    commit: Option<String>,
    /// Also split a commit a remote-tracking branch already has.
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit split` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_split(args: SplitArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::merge::split::split(args.commit, args.force)
}
