//! Command-line interface of `rgit refs`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit refs`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/refs_cmd/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_refs`
#[derive(clap::Args, Debug)]
pub struct RefsArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit refs` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_refs(args: RefsArgs) -> anyhow::Result<()> {
    let RefsArgs { args } = args;
    crate::references::refs_cmd::refs(crate::cli::raw_args_after("refs", args))?;
    Ok(())
}
