//! Command-line interface of `rgit show-ref`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit show-ref`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/show_ref/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_show_ref`
#[derive(clap::Args, Debug)]
pub struct ShowRefArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit show-ref` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_show_ref(args: ShowRefArgs) -> anyhow::Result<()> {
    let ShowRefArgs { args } = args;
    crate::references::show_ref::show_ref(crate::cli::raw_args_after("show-ref", args))?;
    Ok(())
}
