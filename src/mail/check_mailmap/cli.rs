//! Command-line interface of `rgit check-mailmap`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit check-mailmap`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/mail/check_mailmap/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_check_mailmap`
#[derive(clap::Args, Debug)]
pub struct CheckMailmapArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit check-mailmap` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_check_mailmap(args: CheckMailmapArgs) -> anyhow::Result<()> {
    let CheckMailmapArgs { args } = args;
    crate::mail::check_mailmap::check_mailmap(crate::cli::raw_args_after("check-mailmap", args))?;
    Ok(())
}
