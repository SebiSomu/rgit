//! Command-line interface of `rgit fixup`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit fixup`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/fixup/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_fixup`
#[derive(clap::Args, Debug)]
pub struct FixupArgs {
    /// The commit to fix (any revision); without it, pick from a list of recent commits.
    commit: Option<String>,
    /// Stage modified and deleted tracked files first, like `commit -a`.
    #[arg(short = 'a', long = "all")]
    all: bool,
    /// Rewrite the commit even if a remote-tracking branch already has it.
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit fixup` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_fixup(args: FixupArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::workspace::fixup::fixup(args.commit, args.all, args.force)
}
