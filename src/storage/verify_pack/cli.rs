//! Command-line interface of `rgit verify-pack`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit verify-pack`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/verify_pack/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_verify_pack`
#[derive(clap::Args, Debug)]
pub struct VerifyPackArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit verify-pack` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_verify_pack(args: VerifyPackArgs) -> anyhow::Result<()> {
    let VerifyPackArgs { args } = args;
    crate::storage::verify_pack::verify_pack(crate::cli::raw_args_after("verify-pack", args))?;
    Ok(())
}
