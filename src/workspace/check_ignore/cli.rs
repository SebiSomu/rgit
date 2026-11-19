//! Command-line interface of `rgit check-ignore`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit check-ignore`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/check_ignore/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_check_ignore`
#[derive(clap::Args, Debug)]
pub struct CheckIgnoreArgs {
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
    #[arg(short = 'n', long = "non-matching")]
    non_matching: bool,
    #[arg(long = "no-index")]
    no_index: bool,
    #[arg(long = "stdin")]
    stdin: bool,
    #[arg(short = 'z')]
    null_terminated: bool,
    #[arg(value_name = "PATHNAME")]
    paths: Vec<String>,
}

/// Runs `rgit check-ignore` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_check_ignore(args: CheckIgnoreArgs) -> anyhow::Result<()> {
    let CheckIgnoreArgs {
        quiet,
        verbose,
        non_matching,
        no_index,
        stdin,
        null_terminated,
        paths,
    } = args;
    let opts = crate::workspace::check_ignore::types::CheckIgnoreOptions {
        quiet,
        verbose,
        non_matching,
        no_index,
        stdin,
        null_terminated,
        paths,
    };
    crate::workspace::check_ignore::check_ignore(opts)?;
    Ok(())
}
