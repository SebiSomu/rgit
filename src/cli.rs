use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Init,
    HashObject {
        #[arg(short = 'w')]
        write: bool,
        file: PathBuf,
    },
    CatFile {
        #[arg(short = 'p')]
        pretty_print: bool,
        object_hash: String,
    },
    WriteTree,
    Add {
        paths: Vec<PathBuf>,
    },
    LsTree {
        #[arg(long)]
        name_only: bool,
        tree_hash: String,
    },
    CommitTree {
        tree_hash: String,
        #[arg(short = 'p')]
        parent_hash: Option<String>,
        #[arg(short = 'm')]
        message: String,
    },
    Commit {
        #[arg(short = 'm', long = "message")]
        message: String,
    },
    Log {
        #[arg(short = 'o', long = "oneline")]
        oneline: bool,
    },
    Status,
    Branch {
        name: Option<String>,
        #[arg(short = 'd', long = "delete", conflicts_with = "force_delete")]
        delete: bool,
        #[arg(short = 'D', conflicts_with = "delete")]
        force_delete: bool,
        #[arg(short = 'm', long = "move", value_name = "NEW")]
        rename: Option<String>,
    },
    Switch {
        branch: String,
        #[arg(short = 'c', long = "create", conflicts_with = "force")]
        create: bool,
        #[arg(long = "detach", conflicts_with = "create")]
        detach: bool,
        #[arg(short = 'f', long = "force", conflicts_with = "create")]
        force: bool,
    },
    Checkout {
        target: Option<String>,
        #[arg(short = 'b')]
        create_branch: Option<String>,
        #[arg(long = "detach")]
        detach: bool,
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
    Restore {
        files: Vec<PathBuf>,
        #[arg(short = 'S', long = "staged")]
        staged: bool,
        #[arg(short = 'W', long = "worktree")]
        worktree: bool,
        #[arg(long = "source")]
        source: Option<String>,
    },
    Diff {
        #[arg(long = "staged", alias = "cached")]
        staged: bool,
        commits: Vec<String>,
        #[arg(last = true)]
        paths: Vec<PathBuf>,
    },
    Merge {
        branch: String,
    },
    Rm {
        files: Vec<PathBuf>,
        #[arg(short = 'f', long = "force")]
        force: bool,
        #[arg(long = "cached")]
        cached: bool,
        #[arg(short = 'r', long = "recursive")]
        recursive: bool,
    },
    Reset {
        commit: Option<String>,
        #[arg(long = "soft", conflicts_with_all = ["mixed", "hard"])]
        soft: bool,
        #[arg(long = "mixed", conflicts_with_all = ["soft", "hard"])]
        mixed: bool,
        #[arg(long = "hard", conflicts_with_all = ["soft", "mixed"])]
        hard: bool,
        #[arg(last = true)]
        paths: Vec<PathBuf>,
    },
    Clean {
        #[arg(short = 'n', long = "dry-run")]
        dry_run: bool,
        #[arg(short = 'f', long = "force")]
        force: bool,
        #[arg(short = 'd')]
        dirs: bool,
        #[arg(short = 'x', conflicts_with = "only_ignored")]
        ignored: bool,
        #[arg(short = 'X', conflicts_with = "ignored")]
        only_ignored: bool,
    },
    CherryPick {
        commit: Option<String>,
        #[arg(short = 'n', long = "no-commit", conflicts_with_all = ["cont", "abort"])]
        no_commit: bool,
        #[arg(long = "continue", conflicts_with_all = ["abort", "no_commit"])]
        cont: bool,
        #[arg(long = "abort", conflicts_with_all = ["cont", "no_commit"])]
        abort: bool,
    },
    Revert {
        commit: Option<String>,
        #[arg(short = 'n', long = "no-commit", conflicts_with_all = ["cont", "abort"])]
        no_commit: bool,
        #[arg(long = "continue", conflicts_with_all = ["abort", "no_commit"])]
        cont: bool,
        #[arg(long = "abort", conflicts_with_all = ["cont", "no_commit"])]
        abort: bool,
    },
    Stash {
        #[command(subcommand)]
        action: Option<StashAction>,
    },
    Bisect {
        #[command(subcommand)]
        action: BisectAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum BisectAction {
    Start {
        bad: Option<String>,
        good: Vec<String>,
    },
    Bad {
        rev: Option<String>,
    },
    Good {
        rev: Option<String>,
    },
    Skip {
        revs: Vec<String>,
    },
    Reset {
        commit: Option<String>,
    },
    Log,
    Run {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum StashAction {
    Push {
        #[arg(short = 'm', long = "message")]
        message: Option<String>,
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
        stash: Option<String>,
    },
    Clear,
}

pub fn handle_commands() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            crate::commands::init()?;
        }
        Commands::HashObject { write, file} => {
            crate::commands::hash_object(write, file)?;
        }
        Commands::CatFile { pretty_print, object_hash } => {
            crate::commands::cat_file(pretty_print, object_hash)?;
        }
        Commands::WriteTree => {
            crate::commands::write_tree()?;
        }
        Commands::Add { paths } => {
            crate::commands::add(paths)?;
        }
        Commands::LsTree { name_only, tree_hash } => {
            crate::commands::ls_tree(name_only, tree_hash)?;
        }
        Commands::CommitTree { tree_hash, parent_hash, message } => {
            crate::commands::commit_tree(tree_hash, parent_hash, message)?;
        }
        Commands::Commit { message } => {
            crate::commands::commit(message)?;
        }
        Commands::Log { oneline } => {
            crate::commands::log(oneline)?;
        }
        Commands::Status => {
            crate::commands::status()?;
        }
        Commands::Branch { name, delete, force_delete, rename } => {
            crate::commands::branch(name, delete, force_delete, rename)?;
        }
        Commands::Switch { branch, create, detach, force } => {
            crate::commands::switch(branch, create, detach, force)?;
        }
        Commands::Checkout { target, create_branch, detach, force } => {
            crate::commands::checkout(target, create_branch, detach, force)?;
        }
        Commands::Restore { files, staged, worktree, source } => {
            crate::commands::restore(files, staged, worktree, source)?;
        }
        Commands::Diff { staged, commits, paths } => {
            let (commit_a, commit_b) = match commits.len() {
                0 => (None, None),
                1 => (Some(commits[0].clone()), None),
                2 => (Some(commits[0].clone()), Some(commits[1].clone())),
                _ => anyhow::bail!("usage: rgit diff [<commit> [<commit>]] [-- <path>...]"),
            };
            crate::commands::diff(staged, commit_a, commit_b, paths)?;
        }
        Commands::Merge { branch } => {
            crate::commands::merge(branch)?;
        }
        Commands::Rm { files, force, cached, recursive } => {
            crate::commands::rm(files, force, cached, recursive)?;
        }
        Commands::Reset { commit, soft, mixed, hard, paths } => {
            crate::commands::reset(commit, soft, mixed, hard, paths)?;
        }
        Commands::Clean { dry_run, force, dirs, ignored, only_ignored } => {
            crate::commands::clean(dry_run, force, dirs, ignored, only_ignored)?;
        }
        Commands::CherryPick { commit, no_commit, cont, abort } => {
            crate::commands::cherry_pick(commit, no_commit, cont, abort)?;
        }
        Commands::Revert { commit, no_commit, cont, abort } => {
            crate::commands::revert(commit, no_commit, cont, abort)?;
        }
        Commands::Stash { action } => {
            crate::commands::stash(action)?;
        }
        Commands::Bisect { action } => {
            crate::commands::bisect(action)?;
        }
    }

    Ok(())
}