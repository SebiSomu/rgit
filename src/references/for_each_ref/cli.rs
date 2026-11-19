//! Command-line interface of `rgit for-each-ref`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit for-each-ref`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/for_each_ref/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_for_each_ref`
#[derive(clap::Args, Debug)]
pub struct ForEachRefArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit for-each-ref` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_for_each_ref(args: ForEachRefArgs) -> anyhow::Result<()> {
    let ForEachRefArgs { args } = args;
    crate::references::for_each_ref::for_each_ref(crate::cli::raw_args_after("for-each-ref", args))?;
    Ok(())
}
