//! Command-line interface of `rgit restore`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use std::path::PathBuf;

// Command-line arguments of `rgit restore`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/restore/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_restore`
#[derive(clap::Args, Debug)]
pub struct RestoreArgs {
    #[arg(short = 'p', long = "patch")]
    patch: bool,
    files: Vec<PathBuf>,
    #[arg(short = 'S', long = "staged")]
    staged: bool,
    #[arg(short = 'W', long = "worktree")]
    worktree: bool,
    #[arg(long = "source")]
    source: Option<String>,
    /// For unmerged paths, take stage 2 (ours) / stage 3 (theirs).
    #[arg(long = "ours", overrides_with = "theirs")]
    ours: bool,
    #[arg(long = "theirs", overrides_with = "ours")]
    theirs: bool,
    /// Recreate the conflicted merge of unmerged paths.
    #[arg(short = 'm', long = "merge")]
    merge: bool,
    /// Like `--merge`, with `merge`, `diff3` or `zdiff3` markers.
    #[arg(long = "conflict", value_name = "STYLE")]
    conflict: Option<String>,
}

/// Runs `rgit restore` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_restore(args: RestoreArgs) -> anyhow::Result<()> {
    match args {
        RestoreArgs { patch: true, files, staged, worktree, source, .. } => {
            crate::workspace::patch::restore_patch(files.iter().map(|p| p.to_string_lossy().to_string()).collect(), staged, worktree, source)?;
        }
        RestoreArgs { patch: false, files, staged, worktree, source, ours, theirs, merge, conflict } => {
            let stage = crate::workspace::restore::writeout_stage(ours, theirs, merge, conflict)?;
            crate::workspace::restore::restore(files, staged, worktree, source, stage)?;
        }
    }
    Ok(())
}
