use clap::Subcommand;

/// Enum of subcommands supported by the stash command.
// Used by: src/cli.rs, src/workspace/stash.rs
// Used by command/helper: CLI argument parser and the `stash` command dispatcher
#[derive(Subcommand, Debug)]
pub enum StashAction {
    Push {
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        /// `-u`: also stash (and then remove) untracked files.
        #[arg(short = 'u', long = "include-untracked", conflicts_with = "all")]
        include_untracked: bool,
        /// `-a`: also stash untracked and ignored files.
        #[arg(short = 'a', long = "all")]
        all: bool,
        #[arg(last = true)]
        paths: Vec<String>,
    },
    Pop {
        stash: Option<String>,
    },
    Apply {
        stash: Option<String>,
    },
    List,
    Drop {
        stash: Option<String>,
    },
    Show {
        // Accepted for git compatibility: `stash show` already prints the full patch.
        #[arg(short = 'p', long = "patch")]
        patch: bool,
        stash: Option<String>,
    },
    Clear,
}
