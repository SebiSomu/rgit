//! Command-line interface of `rgit config`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit config`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/config/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_config`
#[derive(clap::Args, Debug)]
pub struct ConfigArgs {
    key: Option<String>,
    value: Option<String>,
    #[arg(long = "global")]
    global: bool,
    #[arg(short = 'l', long = "list", conflicts_with_all = ["value", "unset"])]
    list: bool,
    #[arg(long = "unset", conflicts_with = "value")]
    unset: bool,
}

/// Runs `rgit config` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_config(args: ConfigArgs) -> anyhow::Result<()> {
    let ConfigArgs { key, value, global, list, unset } = args;
    crate::plumbing::config::config(key, value, global, list, unset)?;
    Ok(())
}
