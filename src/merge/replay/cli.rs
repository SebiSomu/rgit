//! Command-line interface of `rgit replay`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit replay`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/replay/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_replay`
#[derive(clap::Args, Debug)]
pub struct ReplayArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit replay` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_replay(args: ReplayArgs) -> anyhow::Result<()> {
    let ReplayArgs { args } = args;
    crate::merge::replay::replay(crate::cli::raw_args_after("replay", args))?;
    Ok(())
}
