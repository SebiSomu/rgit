//! Command-line interface of `rgit prune`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit prune`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/prune/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_prune`
#[derive(clap::Args, Debug)]
pub struct PruneArgs {
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
    /// Accepted for compatibility; prune shows no progress
    #[arg(long = "progress")]
    progress: bool,
    #[arg(long = "expire", value_name = "TIME")]
    expire: Option<String>,
    #[arg(value_name = "HEAD")]
    heads: Vec<String>,
}

/// Runs `rgit prune` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_prune(args: PruneArgs) -> anyhow::Result<()> {
    let PruneArgs { dry_run, verbose, progress: _, expire, heads } = args;
    crate::storage::prune::prune(dry_run, verbose, expire, heads)?;
    Ok(())
}
