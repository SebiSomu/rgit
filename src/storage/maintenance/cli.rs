//! Command-line interface of `rgit maintenance`, `rgit for-each-repo`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

// Command-line arguments of `rgit maintenance`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/maintenance/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_maintenance`
#[derive(clap::Args, Debug)]
pub struct MaintenanceArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit maintenance` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_maintenance(args: MaintenanceArgs) -> anyhow::Result<()> {
    let MaintenanceArgs { args } = args;
    crate::storage::maintenance::maintenance(crate::cli::raw_args_after("maintenance", args))?;
    Ok(())
}

// Command-line arguments of `rgit for-each-repo`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/maintenance/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_for_each_repo`
#[derive(clap::Args, Debug)]
pub struct ForEachRepoArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit for-each-repo` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_for_each_repo(args: ForEachRepoArgs) -> anyhow::Result<()> {
    let ForEachRepoArgs { args } = args;
    crate::storage::maintenance::for_each_repo(crate::cli::raw_args_after("for-each-repo", args))?;
    Ok(())
}
