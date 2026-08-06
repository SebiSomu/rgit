pub mod cli;
pub mod objects;
pub mod commands;
mod helpers;
mod index;
pub mod refs;

use anyhow::Result;

fn main() -> Result<()> {
    cli::handle_commands()
}