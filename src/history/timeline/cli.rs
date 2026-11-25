//! Command-line interface of `rgit timeline`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit timeline`, parsed by clap through `cli::Commands` (a `///`
// doc comment here would become the command's help text).
// Used by: src/cli.rs, src/history/timeline/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_timeline`
#[derive(clap::Args, Debug)]
pub struct TimelineArgs {
    /// How many days with commits to show (newest first).
    #[arg(short = 'n', long = "days", default_value_t = 14, value_name = "N")]
    days: usize,
    /// Only commits since this date (`2024-01-01`, `3 months ago`, ...).
    #[arg(long = "since", value_name = "DATE")]
    since: Option<String>,
    /// Only the current branch (default: every local branch).
    #[arg(long = "current")]
    current: bool,
}

/// Runs `rgit timeline` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_timeline(args: TimelineArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    let since = match &args.since {
        Some(text) => match crate::core::date::parse_approx_date(text) {
            Some(ts) => Some(ts),
            None => crate::ui::die_fatal(format!("'{}' is not a date rgit understands (try 2024-01-01 or '3 months ago')", text)),
        },
        None => None,
    };
    crate::history::timeline::timeline(args.days.max(1), since, args.current)
}
