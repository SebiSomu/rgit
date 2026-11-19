//! Command-line interface of `rgit request-pull`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit request-pull`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/mail/request_pull/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_request_pull`
#[derive(clap::Args, Debug)]
pub struct RequestPullArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit request-pull` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_request_pull(args: RequestPullArgs) -> anyhow::Result<()> {
    let RequestPullArgs { args } = args;
    crate::mail::request_pull::request_pull(crate::cli::raw_args_after("request-pull", args))?;
    Ok(())
}
