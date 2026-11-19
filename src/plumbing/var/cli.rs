//! Command-line interface of `rgit var`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit var`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/var/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_var`
#[derive(clap::Args, Debug)]
pub struct VarArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit var` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_var(args: VarArgs) -> anyhow::Result<()> {
    let VarArgs { args } = args;
    crate::plumbing::var::var(crate::cli::raw_args_after("var", args))?;
    Ok(())
}
