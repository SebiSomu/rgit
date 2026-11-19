//! Command-line interface of `rgit blame`, `rgit annotate`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

// Command-line arguments of `rgit blame`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/blame/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_blame`
#[derive(clap::Args, Debug)]
pub struct BlameArgs {
    #[arg(short = 'L', value_name = "START,END")]
    line_range: Option<String>,
    #[arg(required = true, num_args = 1..=2)]
    args: Vec<String>,
}

/// Runs `rgit blame` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_blame(args: BlameArgs) -> anyhow::Result<()> {
    let BlameArgs { line_range, args } = args;
    crate::history::blame::blame(args, line_range)?;
    Ok(())
}

// Command-line arguments of `rgit annotate`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/blame/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_annotate`
#[derive(clap::Args, Debug)]
pub struct AnnotateArgs {
    #[arg(short = 'L', value_name = "START,END")]
    line_range: Option<String>,
    #[arg(required = true, num_args = 1..=2)]
    args: Vec<String>,
}

/// Runs `rgit annotate` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_annotate(args: AnnotateArgs) -> anyhow::Result<()> {
    let AnnotateArgs { line_range, args } = args;
    crate::history::blame::annotate(args, line_range)?;
    Ok(())
}
