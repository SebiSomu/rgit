//! Command-line interface of `rgit pr-summary`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit pr-summary`, parsed by clap through `cli::Commands` (a `///`
// doc comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/pr_summary/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_pr_summary`
#[derive(clap::Args, Debug)]
pub struct PrSummaryArgs {
    /// The branch the pull request goes into (default: the remote's default branch, else main or master).
    #[arg(value_name = "BASE")]
    base: Option<String>,
    /// The branch or commit with the changes (default: HEAD).
    #[arg(value_name = "HEAD")]
    head: Option<String>,
    /// The title to use instead of the one rgit makes up.
    #[arg(long = "title", value_name = "TEXT")]
    title: Option<String>,
}

/// Runs `rgit pr-summary` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_pr_summary(args: PrSummaryArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::remote::pr_summary::pr_summary(args.base, args.head, args.title)
}
