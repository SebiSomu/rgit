pub mod cli;
pub mod objects;
pub mod commands;

use anyhow::Result;

fn main() -> Result<()> {
    cli::handle_commands()
}