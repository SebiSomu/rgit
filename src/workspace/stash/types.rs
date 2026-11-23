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

/// What `stash push` and `rgit wip` saved before touching the working tree: HEAD (with its
/// subject line and flattened tree), the HEAD label used in messages ("main" or "(detached HEAD
/// at abc1234)"), the index commit, the tracked working tree's tree, the untracked-files commit
/// (when untracked files were saved), the untracked paths saved and every path the reset back to
/// HEAD has to touch.
// Used by: src/workspace/stash.rs, src/workspace/stash/types.rs, src/workspace/wip.rs
// Used by command/helper: `capture_snapshot`, `clear_saved_changes`, `stash push` and `rgit wip`
#[derive(Debug, Clone)]
pub struct StashSnapshot {
    pub head: String,
    pub head_subject: String,
    pub head_tree: std::collections::BTreeMap<String, ([u8; 20], u32)>,
    pub branch_label: String,
    pub index_commit: String,
    pub working_tree: String,
    pub untracked_commit: Option<String>,
    pub untracked_files: Vec<String>,
    pub tracked_paths: std::collections::BTreeSet<String>,
}
