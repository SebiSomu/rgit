//! Command-line interface of `rgit sparse-checkout`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit sparse-checkout`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/sparse_checkout/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_sparse_checkout`
#[derive(clap::Args, Debug)]
pub struct SparseCheckoutArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit sparse-checkout` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_sparse_checkout(args: SparseCheckoutArgs) -> anyhow::Result<()> {
    let SparseCheckoutArgs { args } = args;
    crate::workspace::sparse_checkout::sparse_checkout(crate::cli::raw_args_after("sparse-checkout", args))?;
    Ok(())
}
