//! Command-line interface of `rgit absorb`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit absorb`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/absorb/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_absorb`
#[derive(clap::Args, Debug)]
pub struct AbsorbArgs {
    /// Print which hunk goes into which commit, change nothing.
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
    /// Also rewrite commits a remote-tracking branch already has.
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit absorb` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_absorb(args: AbsorbArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::workspace::absorb::absorb(args.dry_run, args.force)
}
