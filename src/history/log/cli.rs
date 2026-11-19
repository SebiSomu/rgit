//! Command-line interface of `rgit log`, `rgit show`, `rgit whatchanged`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

// Command-line arguments of `rgit log`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/log/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_log`
#[derive(clap::Args, Debug)]
pub struct LogArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit log` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_log(args: LogArgs) -> anyhow::Result<()> {
    let LogArgs { args } = args;
    crate::history::log::log(crate::cli::raw_args_after("log", args))?;
    Ok(())
}

// Command-line arguments of `rgit show`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/log/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_show`
#[derive(clap::Args, Debug)]
pub struct ShowArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit show` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_show(args: ShowArgs) -> anyhow::Result<()> {
    let ShowArgs { args } = args;
    crate::history::log::show(crate::cli::raw_args_after("show", args))?;
    Ok(())
}

// Command-line arguments of `rgit whatchanged`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/log/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_whatchanged`
#[derive(clap::Args, Debug)]
pub struct WhatchangedArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit whatchanged` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_whatchanged(args: WhatchangedArgs) -> anyhow::Result<()> {
    let WhatchangedArgs { args } = args;
    crate::history::log::whatchanged(crate::cli::raw_args_after("whatchanged", args))?;
    Ok(())
}
