//! Command-line interface of `rgit symbolic-ref`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit symbolic-ref`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/symbolic_ref/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_symbolic_ref`
#[derive(clap::Args, Debug)]
pub struct SymbolicRefArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit symbolic-ref` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_symbolic_ref(args: SymbolicRefArgs) -> anyhow::Result<()> {
    let SymbolicRefArgs { args } = args;
    crate::references::symbolic_ref::symbolic_ref(crate::cli::raw_args_after("symbolic-ref", args))?;
    Ok(())
}
