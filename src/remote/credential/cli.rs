//! Command-line interface of `rgit credential` and `rgit credential-store`: the clap argument
//! structs and the dispatch functions that `cli.rs` calls.

// Command-line arguments of `rgit credential`, parsed by clap through `cli::Commands` (a `///`
// doc comment here would become the command's help text). The command parses them itself.
// Used by: src/cli.rs, src/remote/credential/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_credential`
#[derive(clap::Args, Debug)]
pub struct CredentialArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit credential` with its arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_credential(args: CredentialArgs) -> anyhow::Result<()> {
    crate::remote::credential::credential(crate::cli::raw_args_after("credential", args.args))
}

// Command-line arguments of `rgit credential-store`, parsed by clap through `cli::Commands`.
// The command parses them itself.
// Used by: src/cli.rs, src/remote/credential/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_credential_store`
#[derive(clap::Args, Debug)]
pub struct CredentialStoreArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit credential-store` with its arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_credential_store(args: CredentialStoreArgs) -> anyhow::Result<()> {
    crate::remote::credential::credential_store(crate::cli::raw_args_after("credential-store", args.args))
}
