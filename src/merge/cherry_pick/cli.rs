//! Command-line interface of `rgit cherry-pick`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit cherry-pick`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/cherry_pick/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_cherry_pick`
#[derive(clap::Args, Debug)]
pub struct CherryPickArgs {
    commits: Vec<String>,
    #[arg(short = 'n', long = "no-commit", overrides_with = "commit")]
    no_commit: bool,
    #[arg(long = "commit", overrides_with = "no_commit")]
    commit: bool,
    #[arg(short = 'x')]
    record_origin: bool,
    #[arg(short = 's', long = "signoff")]
    signoff: bool,
    #[arg(short = 'm', long = "mainline", value_name = "PARENT-NUMBER")]
    mainline: Option<usize>,
    #[arg(long = "ff")]
    ff: bool,
    #[arg(long = "allow-empty")]
    allow_empty: bool,
    #[arg(long = "allow-empty-message")]
    allow_empty_message: bool,
    #[arg(long = "keep-redundant-commits")]
    keep_redundant: bool,
    #[arg(long = "continue", conflicts_with_all = ["skip", "abort", "quit"])]
    cont: bool,
    #[arg(long = "skip", conflicts_with_all = ["cont", "abort", "quit"])]
    skip: bool,
    #[arg(long = "abort", conflicts_with_all = ["cont", "skip", "quit"])]
    abort: bool,
    #[arg(long = "quit", conflicts_with_all = ["cont", "skip", "abort"])]
    quit: bool,
}

/// Runs `rgit cherry-pick` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_cherry_pick(args: CherryPickArgs) -> anyhow::Result<()> {
    let CherryPickArgs { commits, no_commit, commit: _, record_origin, signoff, mainline, ff, allow_empty, allow_empty_message, keep_redundant, cont, skip, abort, quit } = args;
    crate::merge::cherry_pick::cherry_pick(crate::merge::cherry_pick::types::CherryPickOptions { commits, no_commit, record_origin, signoff, mainline, ff, allow_empty, allow_empty_message, keep_redundant, cont, skip, abort, quit })?;
    Ok(())
}
