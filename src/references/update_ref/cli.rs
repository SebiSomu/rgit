//! Command-line interface of `rgit update-ref`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit update-ref`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/update_ref/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_update_ref`
#[derive(clap::Args, Debug)]
pub struct UpdateRefArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit update-ref` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_update_ref(args: UpdateRefArgs) -> anyhow::Result<()> {
    let UpdateRefArgs { args } = args;
    crate::references::update_ref::update_ref(crate::cli::raw_args_after("update-ref", args))?;
    Ok(())
}
