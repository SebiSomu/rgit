//! Command-line interface of `rgit version`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit version`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/version/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_version`
#[derive(clap::Args, Debug)]
pub struct VersionArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit version` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_version(args: VersionArgs) -> anyhow::Result<()> {
    let VersionArgs { args } = args;
    crate::plumbing::version::version(crate::cli::raw_args_after("version", args))?;
    Ok(())
}
