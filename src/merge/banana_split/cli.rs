//! Command-line interface of `rgit banana-split`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit banana-split`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/banana_split/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_banana_split`
#[derive(clap::Args, Debug)]
pub struct BananaSplitArgs {
    commit: Option<String>,
    #[arg(long = "by-dir")]
    by_dir: bool,
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
}

/// Runs `rgit banana-split` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_banana_split(args: BananaSplitArgs) -> anyhow::Result<()> {
    let BananaSplitArgs { commit, by_dir, dry_run } = args;
    crate::merge::banana_split::banana_split(commit, by_dir, dry_run)?;
    Ok(())
}
