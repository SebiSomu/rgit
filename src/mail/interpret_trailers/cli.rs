//! Command-line interface of `rgit interpret-trailers`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit interpret-trailers`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/mail/interpret_trailers/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_interpret_trailers`
#[derive(clap::Args, Debug)]
pub struct InterpretTrailersArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit interpret-trailers` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_interpret_trailers(args: InterpretTrailersArgs) -> anyhow::Result<()> {
    let InterpretTrailersArgs { args } = args;
    crate::mail::interpret_trailers::interpret_trailers(crate::cli::raw_args_after("interpret-trailers", args))?;
    Ok(())
}
