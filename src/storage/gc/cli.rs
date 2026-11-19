//! Command-line interface of `rgit gc`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit gc`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/gc/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_gc`
#[derive(clap::Args, Debug)]
pub struct GcArgs {
    #[arg(long = "prune", default_value = "14")]
    prune: String,
    #[arg(long = "reflog-expire", default_value = "90")]
    reflog_expire: String,
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
}

/// Runs `rgit gc` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_gc(args: GcArgs) -> anyhow::Result<()> {
    let GcArgs { prune, reflog_expire, dry_run } = args;
    crate::storage::gc::gc(prune, reflog_expire, dry_run)?;
    Ok(())
}
