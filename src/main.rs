pub mod cli;
pub mod objects;

use anyhow::Result;

fn main() -> Result<()> {
    cli::handle_commands()
}