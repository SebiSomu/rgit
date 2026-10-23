pub mod cli;
pub mod objects;
pub mod commands;
mod helpers;
mod index;
pub mod refs;
pub mod repo;

use anyhow::Result;

fn main() -> Result<()> {
    let stack_size = 64 * 1024 * 1024; // 64 MB — the Commands enum is large enough to overflow the default 8 MB stack
    let builder = std::thread::Builder::new().stack_size(stack_size);
    let handler = builder.spawn(|| {
        cli::handle_commands()
    }).expect("failed to spawn main thread");

    handler.join().expect("main thread panicked")
}