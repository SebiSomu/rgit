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

    Commit {
        #[arg(short = 'm')]
        message: String,
    },
}

pub fn handle_commands() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            println!("TODO: Implement 'rgit init'");
        }
        Commands::HashObject { write: _write, file: _file } => {
            println!("TODO: Implement 'rgit hash-object'");
        }
        Commands::CatFile { pretty_print: _pretty_print, object_hash: _object_hash } => {
            println!("TODO: Implement 'rgit cat-file'");
        }
        Commands::Commit { message: _message } => {
            println!("TODO: Implement 'rgit commit'");
        }
    }

    Ok(())
}