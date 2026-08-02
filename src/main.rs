pub mod cli;
pub mod objects;
pub mod commands;
mod helpers;

use anyhow::Result;

fn main() -> Result<()> {
    cli::handle_commands()
}