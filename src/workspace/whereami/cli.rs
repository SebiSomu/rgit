//! Command-line interface of `rgit whereami`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit whereami`, parsed by clap through `cli::Commands` (a `///`
// doc comment here would become the command's help text). It takes none.
// Used by: src/cli.rs, src/workspace/whereami/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_whereami`
#[derive(clap::Args, Debug)]
pub struct WhereamiArgs {}

/// Runs `rgit whereami`.
// Used by `cli::handle_commands`.
pub fn dispatch_whereami(_args: WhereamiArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::workspace::whereami::whereami()
}
