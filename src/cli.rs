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
    }

    Ok(())
}