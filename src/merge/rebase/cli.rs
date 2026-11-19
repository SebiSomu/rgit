//! Command-line interface of `rgit rebase`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit rebase`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/rebase/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_rebase`
#[derive(clap::Args, Debug)]
pub struct RebaseArgs {
    upstream: Option<String>,
    branch: Option<String>,
    #[arg(long = "onto", value_name = "NEWBASE", conflicts_with = "keep_base")]
    onto: Option<String>,
    #[arg(long = "keep-base")]
    keep_base: bool,
    #[arg(long = "reapply-cherry-picks", overrides_with = "no_reapply_cherry_picks")]
    reapply_cherry_picks: bool,
    #[arg(long = "no-reapply-cherry-picks", overrides_with = "reapply_cherry_picks")]
    no_reapply_cherry_picks: bool,
    #[arg(short = 'i', long = "interactive", conflicts_with_all = ["cont", "abort", "skip", "edit_todo"])]
    interactive: bool,
    #[arg(long = "continue")]
    cont: bool,
    #[arg(long = "abort")]
    abort: bool,
    #[arg(long = "skip")]
    skip: bool,
    #[arg(long = "edit-todo")]
    edit_todo: bool,
}

/// Runs `rgit rebase` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_rebase(args: RebaseArgs) -> anyhow::Result<()> {
    let RebaseArgs { upstream, branch, onto, keep_base, reapply_cherry_picks, no_reapply_cherry_picks, interactive, cont, abort, skip, edit_todo } = args;
    let reapply_cherry_picks = if reapply_cherry_picks { Some(true) } else if no_reapply_cherry_picks { Some(false) } else { None };
    crate::merge::rebase::rebase(crate::merge::rebase::types::RebaseOptions { upstream, branch, onto, keep_base, reapply_cherry_picks, interactive, cont, abort, skip, edit_todo })?;
    Ok(())
}
