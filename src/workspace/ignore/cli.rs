//! Command-line interface of `rgit ignore`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit ignore`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/ignore/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_ignore`
#[derive(clap::Args, Debug)]
pub struct IgnoreArgs {
    /// Patterns, in .gitignore syntax (quote them so the shell doesn't expand `*`).
    #[arg(required = true, value_name = "PATTERN")]
    patterns: Vec<String>,
    /// Add them to the global ignore file (core.excludesFile, default ~/.config/git/ignore) instead of .gitignore.
    #[arg(long = "global")]
    global: bool,
    /// Keep tracking files that the patterns match (by default they are untracked, kept on disk).
    #[arg(long = "keep-tracked")]
    keep_tracked: bool,
    /// Print what would change, change nothing.
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
}

/// Runs `rgit ignore` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_ignore(args: IgnoreArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::workspace::ignore::ignore(args.patterns, args.global, args.keep_tracked, args.dry_run)
}
