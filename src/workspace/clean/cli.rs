//! Command-line interface of `rgit clean`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit clean`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/clean/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_clean`
#[derive(clap::Args, Debug)]
pub struct CleanArgs {
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
    #[arg(short = 'f', long = "force")]
    force: bool,
    #[arg(short = 'd')]
    dirs: bool,
    #[arg(short = 'x', conflicts_with = "only_ignored")]
    ignored: bool,
    #[arg(short = 'X', conflicts_with = "ignored")]
    only_ignored: bool
}

/// Runs `rgit clean` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_clean(args: CleanArgs) -> anyhow::Result<()> {
    let CleanArgs { dry_run, force, dirs, ignored, only_ignored } = args;
    crate::workspace::clean::clean(dry_run, force, dirs, ignored, only_ignored)?;
    Ok(())
}
