pub mod cli;
pub mod objects;
pub mod commands;
mod helpers;
mod index;

use anyhow::Result;

fn main() -> Result<()> {
    cli::handle_commands()
}