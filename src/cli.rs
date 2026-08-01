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
    }

    Ok(())
}