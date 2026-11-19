//! Command-line interface of `rgit rerere`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit rerere`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/rerere/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_rerere`
#[derive(clap::Args, Debug)]
pub struct RerereArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit rerere` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_rerere(args: RerereArgs) -> anyhow::Result<()> {
    let RerereArgs { args } = args;
    crate::merge::rerere::rerere(crate::cli::raw_args_after("rerere", args))?;
    Ok(())
}
