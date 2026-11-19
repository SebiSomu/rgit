pub mod cli;
pub mod core;
pub mod objects;
pub mod ui;

// Domains: each holds its commands (`<domain>/<command>.rs`) and their engines.
mod diff;
mod export;
mod history;
mod mail;
mod merge;
mod plumbing;
mod references;
mod remote;
mod storage;
mod workspace;

use anyhow::Result;

fn main() -> Result<()> {
    let stack_size = 64 * 1024 * 1024; // 64 MB — the Commands enum is large enough to overflow the default 8 MB stack
    let builder = std::thread::Builder::new().stack_size(stack_size);
    let handler = builder.spawn(|| {
        // After a command that rewrote the working tree, a sparse checkout is applied again.
        let result = cli::handle_commands().and_then(|_| crate::workspace::sparse_checkout::engine::reapply_after_command());
        ui::set_outcome(result.is_ok());
        result
    }).expect("failed to spawn main thread");

    handler.join().expect("main thread panicked")
}