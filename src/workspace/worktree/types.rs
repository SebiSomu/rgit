use clap::Subcommand;
use std::path::PathBuf;

/// Subcommands of `worktree`, which manages extra working trees attached to one repository.
/// Each linked worktree has its own HEAD, index and in-progress state under
/// `.git/worktrees/<id>/`, while objects, refs and config stay shared.
// Used by: src/cli.rs, src/workspace/worktree.rs
// Used by command/helper: CLI argument parser and the `worktree` command dispatcher
#[derive(Subcommand, Debug)]
pub enum WorktreeAction {
    /// Create a worktree at <path> and check out <commit-ish> in it
    Add {
        path: PathBuf,
        commit_ish: Option<String>,
        /// Create a new branch <new-branch> starting at <commit-ish> and check it out
        #[arg(short = 'b', value_name = "new-branch", conflicts_with_all = ["force_new_branch", "detach"])]
        new_branch: Option<String>,
        /// Like -b, but reset <new-branch> to <commit-ish> if it already exists
        #[arg(short = 'B', value_name = "new-branch", conflicts_with = "detach")]
        force_new_branch: Option<String>,
        /// Check out <commit-ish> as a detached HEAD
        #[arg(short = 'd', long = "detach")]
        detach: bool,
        /// Allow a branch that is already checked out elsewhere, or a missing registered path
        #[arg(short = 'f', long = "force")]
        force: bool,
        /// Lock the new worktree right after creating it
        #[arg(long = "lock")]
        lock: bool,
        /// Reason recorded with --lock
        #[arg(long = "reason", requires = "lock")]
        reason: Option<String>,
        #[arg(short = 'q', long = "quiet")]
        quiet: bool,
    },
    /// List the main worktree and every linked worktree
    List {
        #[arg(long = "porcelain")]
        porcelain: bool,
        #[arg(short = 'v', long = "verbose", conflicts_with = "porcelain")]
        verbose: bool,
    },
    /// Delete a linked worktree (refuses a dirty one unless forced)
    Remove {
        worktree: String,
        /// Remove even with local changes; give twice to also remove a locked worktree
        #[arg(short = 'f', long = "force", action = clap::ArgAction::Count)]
        force: u8,
    },
    /// Move a linked worktree to a new location
    Move {
        worktree: String,
        new_path: PathBuf,
        /// Give twice to move a locked worktree
        #[arg(short = 'f', long = "force", action = clap::ArgAction::Count)]
        force: u8,
    },
    /// Protect a linked worktree from being pruned, moved or removed
    Lock {
        worktree: String,
        #[arg(long = "reason")]
        reason: Option<String>,
    },
    /// Undo `worktree lock`
    Unlock {
        worktree: String,
    },
    /// Delete the records of worktrees whose directories no longer exist
    Prune {
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,
    },
}

/// One worktree of the repository as `worktree list` reports it: where it is, what its HEAD
/// points at, and whether it is locked or can be pruned. The main worktree has no `id`.
// Used by: src/workspace/worktree/types.rs, src/workspace/worktree/engine.rs
// Used by command/helper: `list_worktrees`, `find_worktree`, `worktree_using_branch`, and the `worktree` command
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Absolute top directory of the worktree.
    pub path: PathBuf,
    /// Directory holding its HEAD and index: the shared `.git` for the main worktree,
    /// `.git/worktrees/<id>` for a linked one.
    pub admin_dir: PathBuf,
    /// Name of the linked worktree's directory under `.git/worktrees/`; `None` for the main one.
    pub id: Option<String>,
    /// Commit HEAD resolves to; `None` on an unborn branch.
    pub head: Option<String>,
    /// Full ref HEAD is attached to (`refs/heads/main`); `None` when detached.
    pub branch: Option<String>,
    /// `Some(reason)` when locked; the reason may be empty.
    pub locked: Option<String>,
    /// `Some(reason)` when the worktree's directory is gone and its record can be pruned.
    pub prunable: Option<String>,
    /// Whether the command is running inside this worktree.
    pub is_current: bool,
}

impl WorktreeInfo {
    /// Says whether this is the repository's main worktree.
    pub fn is_main(&self) -> bool {
        self.id.is_none()
    }

    /// The path the way git prints it: forward slashes on every platform.
    pub fn display_path(&self) -> String {
        crate::core::repo::to_git_path_string(&self.path)
    }
}

/// How `worktree add` comes by the branch it checks out in the new worktree.
// Used by: src/workspace/worktree.rs
// Used by command/helper: the `worktree add` command
pub enum BranchCreation {
    /// Already exists (or there is no branch: detached HEAD).
    Existing,
    /// Created by this command (`-b`, or the branch named after the path).
    New,
    /// `-B` moved an existing branch; holds its previous tip.
    Reset(String),
}
