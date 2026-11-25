//! Command-line interface of `rgit tip` (also `rgit bingo`) and `rgit zen`: the clap argument
//! structs and the dispatch functions that `cli.rs` calls.

// Command-line arguments of `rgit tip`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/tip/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_tip`
#[derive(clap::Args, Debug)]
pub struct TipArgs {
    /// Show this tip (by number) instead of the tip of the day.
    #[arg(value_name = "N", conflicts_with_all = ["all", "random"])]
    number: Option<usize>,
    /// A random tip instead of the tip of the day.
    #[arg(short = 'r', long = "random", conflicts_with = "all")]
    random: bool,
    /// List every tip's title.
    #[arg(long = "all")]
    all: bool,
}

// Command-line arguments of `rgit zen` (none).
// Used by: src/cli.rs, src/plumbing/tip/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_zen`
#[derive(clap::Args, Debug)]
pub struct ZenArgs {}

/// Runs `rgit tip` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_tip(args: TipArgs) -> anyhow::Result<()> {
    crate::plumbing::tip::tip(args.number, args.random, args.all)
}

/// Runs `rgit zen`.
// Used by `cli::handle_commands`.
pub fn dispatch_zen(_args: ZenArgs) -> anyhow::Result<()> {
    print!("{}", crate::plumbing::tip::engine::ZEN);
    Ok(())
}
