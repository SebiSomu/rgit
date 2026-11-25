//! Command-line interface of `rgit backup`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit backup`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/backup/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_backup`
#[derive(clap::Args, Debug)]
pub struct BackupArgs {
    /// Where the backups go (default: rgit.backupDir, else ~/.rgit-backups).
    #[arg(long = "dir", value_name = "DIR")]
    dir: Option<String>,
    /// How many backups of this repository to keep, newest first; 0 keeps all (default: rgit.backupKeep, else 10).
    #[arg(long = "keep", value_name = "N")]
    keep: Option<usize>,
    /// Back up even when nothing changed since the last backup.
    #[arg(short = 'f', long = "force")]
    force: bool,
    /// List this repository's backups instead.
    #[arg(long = "list", conflicts_with_all = ["force", "keep"])]
    list: bool,
}

/// Runs `rgit backup` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_backup(args: BackupArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::remote::backup::backup(args.dir, args.keep, args.force, args.list)
}
