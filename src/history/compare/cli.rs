//! Command-line interface of `rgit compare`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit compare`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/compare/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_compare`
#[derive(clap::Args, Debug)]
pub struct CompareArgs {
    /// Two branches or commits; with one, HEAD against it; with none, HEAD against its upstream.
    #[arg(value_name = "REV", num_args = 0..=2)]
    revs: Vec<String>,
    /// How many commits each side's list shows.
    #[arg(short = 'n', long = "max-count", default_value_t = 10, value_name = "N")]
    max_count: usize,
}

/// Runs `rgit compare` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_compare(args: CompareArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::history::compare::compare(args.revs, args.max_count)
}
