//! Command-line interface of `rgit fsck`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit fsck`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/fsck/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_fsck`
#[derive(clap::Args, Debug)]
pub struct FsckArgs {
    #[arg(long = "connectivity-only")]
    connectivity_only: bool,
    #[arg(long = "no-dangling")]
    no_dangling: bool,
    #[arg(long = "unreachable")]
    unreachable: bool,
    #[arg(long = "no-reflogs")]
    no_reflogs: bool,
}

/// Runs `rgit fsck` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_fsck(args: FsckArgs) -> anyhow::Result<()> {
    let FsckArgs { connectivity_only, no_dangling, unreachable, no_reflogs } = args;
    crate::storage::fsck::fsck(connectivity_only, no_dangling, unreachable, no_reflogs)?;
    Ok(())
}
