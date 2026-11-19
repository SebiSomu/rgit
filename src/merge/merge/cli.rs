//! Command-line interface of `rgit merge`, `rgit merge-base`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

// Command-line arguments of `rgit merge`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/merge/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_merge`
#[derive(clap::Args, Debug)]
pub struct MergeArgs {
    #[arg(long = "ff", overrides_with_all = ["no_ff", "ff_only"])]
    ff: bool,
    #[arg(long = "no-ff", overrides_with_all = ["ff", "ff_only"])]
    no_ff: bool,
    #[arg(long = "ff-only", overrides_with_all = ["ff", "no_ff"])]
    ff_only: bool,
    #[arg(long = "squash", overrides_with = "no_squash")]
    squash: bool,
    #[arg(long = "no-squash", overrides_with = "squash")]
    no_squash: bool,
    #[arg(long = "commit", overrides_with = "no_commit")]
    commit: bool,
    #[arg(long = "no-commit", overrides_with = "commit")]
    no_commit: bool,
    #[arg(short = 'm', long = "message", value_name = "MESSAGE")]
    messages: Vec<String>,
    #[arg(short = 'F', long = "file", value_name = "FILE")]
    file: Option<String>,
    #[arg(short = 's', long = "strategy", value_name = "STRATEGY")]
    strategy: Option<String>,
    #[arg(short = 'X', long = "strategy-option", value_name = "OPTION")]
    strategy_options: Vec<String>,
    #[arg(long = "abort")]
    abort: bool,
    #[arg(long = "continue")]
    cont: bool,
    #[arg(long = "quit")]
    quit: bool,
    #[arg(short = 'n', long = "no-stat", overrides_with = "stat")]
    no_stat: bool,
    #[arg(long = "stat", overrides_with = "no_stat")]
    stat: bool,
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
    /// Accepted for compatibility: rgit never opens an editor for a merge commit.
    #[arg(long = "no-edit")]
    no_edit: bool,
    #[arg(long = "allow-unrelated-histories")]
    allow_unrelated: bool,
    commits: Vec<String>,
}

/// Runs `rgit merge` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_merge(args: MergeArgs) -> anyhow::Result<()> {
    let MergeArgs { ff: _, no_ff, ff_only, squash, no_squash: _, commit, no_commit, messages, file, strategy, strategy_options, abort, cont, quit, no_stat, stat: _, quiet, no_edit: _, allow_unrelated, commits } = args;
    use crate::merge::merge::types::{FastForwardMode, MergeOptions};
    let ff = if no_ff { FastForwardMode::Never } else if ff_only { FastForwardMode::Only } else { FastForwardMode::Allow };
    let commit = if commit { Some(true) } else if no_commit { Some(false) } else { None };
    crate::merge::merge::merge(MergeOptions { commits, ff, squash, commit, messages, file, strategy, strategy_options, abort, cont, quit, no_stat, quiet, allow_unrelated })?;
    Ok(())
}

// Command-line arguments of `rgit merge-base`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/merge/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_merge_base`
#[derive(clap::Args, Debug)]
pub struct MergeBaseArgs {
    #[arg(short = 'a', long = "all", conflicts_with = "is_ancestor")]
    all: bool,
    #[arg(long = "is-ancestor", conflicts_with_all = ["all", "octopus"])]
    is_ancestor: bool,
    #[arg(long = "octopus", conflicts_with = "is_ancestor")]
    octopus: bool,
    #[arg(required = true)]
    commits: Vec<String>,
}

/// Runs `rgit merge-base` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_merge_base(args: MergeBaseArgs) -> anyhow::Result<()> {
    let MergeBaseArgs { all, is_ancestor, octopus, commits } = args;
    crate::merge::merge::merge_base(all, is_ancestor, octopus, commits)?;
    Ok(())
}
