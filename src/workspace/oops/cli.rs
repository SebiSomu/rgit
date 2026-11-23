//! Command-line interface of `rgit oops`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit oops`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/oops/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_oops`
#[derive(clap::Args, Debug)]
pub struct OopsArgs {
    /// Run this suggestion (its number in the list) instead of listing them.
    #[arg(value_name = "NUMBER")]
    pick: Option<usize>,
    /// List every suggestion, not just the three most likely.
    #[arg(long = "all", conflicts_with = "pick")]
    all: bool,
}

/// Runs `rgit oops` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_oops(args: OopsArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::workspace::oops::oops(args.pick, args.all)
}
