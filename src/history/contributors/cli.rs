//! Command-line interface of `rgit contributors`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use crate::history::contributors::types::SortBy;

// Command-line arguments of `rgit contributors`, parsed by clap through `cli::Commands` (a
// `///` doc comment here would become the command's help text).
// Used by: src/cli.rs, src/history/contributors/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_contributors`
#[derive(clap::Args, Debug)]
pub struct ContributorsArgs {
    /// Revisions or a range (`v1.0..`); default: HEAD.
    #[arg(value_name = "REV")]
    revs: Vec<String>,
    /// Only commits since this date (`2024-01-01`, `3 months ago`, ...).
    #[arg(long = "since", value_name = "DATE")]
    since: Option<String>,
    /// Order of the table.
    #[arg(long = "sort", value_enum, default_value_t = SortBy::Commits)]
    sort: SortBy,
    /// Show each contributor's email.
    #[arg(short = 'e', long = "email")]
    email: bool,
}

/// Runs `rgit contributors` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_contributors(args: ContributorsArgs) -> anyhow::Result<()> {
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
    crate::history::contributors::contributors(args.revs, since, args.sort, args.email)
}
