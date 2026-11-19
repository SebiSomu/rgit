//! Command-line interface of `rgit count-objects`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit count-objects`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/count_objects/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_count_objects`
#[derive(clap::Args, Debug)]
pub struct CountObjectsArgs {
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
    #[arg(short = 'H', long = "human-readable")]
    human_readable: bool,
}

/// Runs `rgit count-objects` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_count_objects(args: CountObjectsArgs) -> anyhow::Result<()> {
    let CountObjectsArgs { verbose, human_readable } = args;
    crate::storage::count_objects::count_objects(verbose, human_readable)?;
    Ok(())
}
