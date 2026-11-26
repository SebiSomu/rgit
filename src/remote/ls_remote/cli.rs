//! Command-line interface of `rgit ls-remote`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit ls-remote`, parsed by clap through `cli::Commands` (a `///`
// doc comment here would become the command's help text). The command parses them itself.
// Used by: src/cli.rs, src/remote/ls_remote/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_ls_remote`
#[derive(clap::Args, Debug)]
pub struct LsRemoteArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit ls-remote` with its arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_ls_remote(args: LsRemoteArgs) -> anyhow::Result<()> {
    crate::remote::ls_remote::ls_remote(crate::cli::raw_args_after("ls-remote", args.args))
}
