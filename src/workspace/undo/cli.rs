//! Command-line interface of `rgit undo` and `rgit redo`: the clap argument structs and the
//! dispatch functions that `cli.rs` calls.

// Command-line arguments of `rgit undo`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/undo/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_undo`
#[derive(clap::Args, Debug)]
pub struct UndoArgs {
    /// List the recorded operations instead of undoing one.
    #[arg(long = "list", conflicts_with_all = ["dry_run", "force"])]
    list: bool,
    /// Show what would be undone without changing anything.
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
    /// Undo even if something the operation touched has changed since (overwrites it).
    #[arg(short = 'f', long = "force")]
    force: bool,
}

// Command-line arguments of `rgit redo`, parsed by clap through `cli::Commands`.
// Used by: src/cli.rs, src/workspace/undo/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_redo`
#[derive(clap::Args, Debug)]
pub struct RedoArgs {
    /// Show what would be redone without changing anything.
    #[arg(short = 'n', long = "dry-run")]
    dry_run: bool,
    /// Redo even if something the operation touched has changed since (overwrites it).
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit undo` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_undo(args: UndoArgs) -> anyhow::Result<()> {
    require_repo()?;
    if args.list {
        return crate::workspace::undo::list();
    }
    crate::workspace::undo::undo(args.dry_run, args.force)
}

/// Runs `rgit redo` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_redo(args: RedoArgs) -> anyhow::Result<()> {
    require_repo()?;
    crate::workspace::undo::redo(args.dry_run, args.force)
}

fn require_repo() -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    Ok(())
}
