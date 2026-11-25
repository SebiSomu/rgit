//! Command-line interface of `rgit secrets`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

use crate::workspace::secrets::types::Scope;

// Command-line arguments of `rgit secrets`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/secrets/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_secrets`
#[derive(clap::Args, Debug)]
pub struct SecretsArgs {
    /// Scan every tracked file (as staged), not only the staged changes.
    #[arg(long = "all-files", conflicts_with = "history")]
    all_files: bool,
    /// Scan the lines every commit added (reachable from HEAD, or from the given revisions/ranges).
    #[arg(long = "history")]
    history: bool,
    /// Revisions or ranges for --history (default: HEAD).
    #[arg(value_name = "REV", requires = "history")]
    revs: Vec<String>,
    /// Print nothing; only the exit code says (1 = something found).
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
}

/// Runs `rgit secrets` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_secrets(args: SecretsArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    let scope = if args.history {
        Scope::History(args.revs)
    } else if args.all_files {
        Scope::AllFiles
    } else {
        Scope::Staged
    };
    crate::workspace::secrets::secrets(scope, args.quiet)
}
