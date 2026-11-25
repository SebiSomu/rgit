//! Command-line interface of `rgit stats`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit stats`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/stats/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_stats`
#[derive(clap::Args, Debug)]
pub struct StatsArgs {
    /// Only commits and files under these paths.
    #[arg(value_name = "PATH")]
    paths: Vec<String>,
    /// Only commits since this date (`2024-01-01`, `3 months ago`, ...).
    #[arg(long = "since", value_name = "DATE")]
    since: Option<String>,
    /// How many weeks the activity line covers (ending at the newest commit).
    #[arg(long = "weeks", default_value_t = 12, value_name = "N")]
    weeks: usize,
}

/// Runs `rgit stats` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_stats(args: StatsArgs) -> anyhow::Result<()> {
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
    crate::history::stats::stats(&args.paths, since, args.weeks.clamp(1, 104))
}
