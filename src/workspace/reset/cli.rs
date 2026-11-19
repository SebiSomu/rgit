//! Command-line interface of `rgit reset`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use std::path::PathBuf;

// Command-line arguments of `rgit reset`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/reset/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_reset`
#[derive(clap::Args, Debug)]
pub struct ResetArgs {
    commit: Option<String>,
    #[arg(long = "soft", conflicts_with_all = ["mixed", "hard"])]
    soft: bool,
    #[arg(long = "mixed", conflicts_with_all = ["soft", "hard"])]
    mixed: bool,
    #[arg(long = "hard", conflicts_with_all = ["soft", "mixed"])]
    hard: bool,
    #[arg(short = 'p', long = "patch")]
    patch: bool,
    #[arg(last = true)]
    paths: Vec<PathBuf>
}

/// Runs `rgit reset` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_reset(args: ResetArgs) -> anyhow::Result<()> {
    match args {
        ResetArgs { patch: true, soft, hard, .. } => {
            if soft || hard {
                anyhow::bail!("fatal: options '--patch' and '--{}' cannot be used together", if soft { "soft" } else { "hard" });
            }
            crate::workspace::patch::reset_patch(crate::cli::raw_args_after("reset", Vec::new()))?;
        }
        ResetArgs { commit, soft, mixed, hard, paths, .. } => {
            crate::workspace::reset::reset(commit, soft, mixed, hard, paths)?;
        }
    }
    Ok(())
}
