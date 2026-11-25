//! Command-line interface of `rgit hotspots`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit hotspots`, parsed by clap through `cli::Commands` (a `///`
// doc comment here would become the command's help text).
// Used by: src/cli.rs, src/history/hotspots/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_hotspots`
#[derive(clap::Args, Debug)]
pub struct HotspotsArgs {
    /// Only files under these paths.
    #[arg(value_name = "PATH")]
    paths: Vec<String>,
    /// Only commits since this date (`2024-01-01`, `3 months ago`, ...).
    #[arg(long = "since", value_name = "DATE")]
    since: Option<String>,
    /// How many entries each list shows.
    #[arg(short = 'n', long = "max-count", default_value_t = 10, value_name = "N")]
    max_count: usize,
}

/// Runs `rgit hotspots` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_hotspots(args: HotspotsArgs) -> anyhow::Result<()> {
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
    crate::history::hotspots::hotspots(&args.paths, since, args.since.as_deref(), args.max_count)
}
