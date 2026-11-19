//! Command-line interface of `rgit cherry`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit cherry`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/cherry/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_cherry`
#[derive(clap::Args, Debug)]
pub struct CherryArgs {
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
    #[arg(long = "abbrev", value_name = "N", num_args = 0..=1, require_equals = true, default_missing_value = "7")]
    abbrev: Option<usize>,
    upstream: Option<String>,
    head: Option<String>,
    limit: Option<String>,
}

/// Runs `rgit cherry` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_cherry(args: CherryArgs) -> anyhow::Result<()> {
    let CherryArgs {
        verbose,
        abbrev,
        upstream,
        head,
        limit,
    } = args;
    let opts = crate::history::cherry::types::CherryOptions {
        verbose,
        abbrev,
        upstream,
        head,
        limit,
    };
    crate::history::cherry::cherry(opts)?;
    Ok(())
}
