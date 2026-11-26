//! Command-line interface of `rgit scrub`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit scrub`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/scrub/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_scrub`
#[derive(clap::Args, Debug)]
pub struct ScrubArgs {
    /// Files or folders to remove from every commit
    #[arg(required_unless_present = "drop_backup")]
    paths: Vec<String>,
    /// Show what would change without writing anything
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
    /// Delete the backup refs a previous scrub left
    #[arg(long = "drop-backup", conflicts_with_all = ["paths", "dry_run"])]
    drop_backup: bool,
}

/// Runs `rgit scrub` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_scrub(args: ScrubArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    if args.drop_backup {
        return crate::history::scrub::drop_scrub_backup();
    }
    crate::history::scrub::scrub(&args.paths, args.dry_run)
}
