//! Command-line interface of `rgit changelog`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit changelog`, parsed by clap through `cli::Commands` (a `///`
// doc comment here would become the command's help text).
// Used by: src/cli.rs, src/history/changelog/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_changelog`
#[derive(clap::Args, Debug)]
pub struct ChangelogArgs {
    /// The commits to cover, as `<a>..<b>` (default: since the latest tag, or everything).
    #[arg(value_name = "RANGE", conflicts_with = "all")]
    range: Option<String>,
    /// The heading's version name (default: `Unreleased`, or the tag the range ends at).
    #[arg(long = "version", value_name = "NAME")]
    version: Option<String>,
    /// One section per tag, for the whole history (newest first).
    #[arg(long = "all")]
    all: bool,
    /// Add the result at the top of this file (after its first `# ` title, if it has one) instead of printing it.
    #[arg(long = "prepend", value_name = "FILE")]
    prepend: Option<String>,
}

/// Runs `rgit changelog` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_changelog(args: ChangelogArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::history::changelog::changelog(args.range, args.version, args.all, args.prepend)
}
