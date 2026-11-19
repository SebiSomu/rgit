//! Command-line interface of `rgit name-rev`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit name-rev`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/name_rev/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_name_rev`
#[derive(clap::Args, Debug)]
pub struct NameRevArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit name-rev` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_name_rev(args: NameRevArgs) -> anyhow::Result<()> {
    let NameRevArgs { args } = args;
    crate::history::name_rev::name_rev(crate::cli::raw_args_after("name-rev", args))?;
    Ok(())
}
