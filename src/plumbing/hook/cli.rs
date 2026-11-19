//! Command-line interface of `rgit hook`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit hook`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/hook/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_hook`
#[derive(clap::Args, Debug)]
pub struct HookArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit hook` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_hook(args: HookArgs) -> anyhow::Result<()> {
    let HookArgs { args } = args;
    crate::plumbing::hook::hook(crate::cli::raw_args_after("hook", args))?;
    Ok(())
}
