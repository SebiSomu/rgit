//! Command-line interface of `rgit bundle`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit bundle`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/bundle/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_bundle`
#[derive(clap::Args, Debug)]
pub struct BundleArgs {
    #[command(subcommand)]
    action: crate::remote::bundle::types::BundleAction,
}

/// Runs `rgit bundle` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_bundle(args: BundleArgs) -> anyhow::Result<()> {
    let BundleArgs { action } = args;
    crate::remote::bundle::bundle(action)?;
    Ok(())
}
