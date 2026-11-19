//! Command-line interface of `rgit checkout-index`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit checkout-index`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/checkout_index/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_checkout_index`
#[derive(clap::Args, Debug)]
pub struct CheckoutIndexArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit checkout-index` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_checkout_index(args: CheckoutIndexArgs) -> anyhow::Result<()> {
    let CheckoutIndexArgs { args } = args;
    crate::workspace::checkout_index::checkout_index(crate::cli::raw_args_after("checkout-index", args))?;
    Ok(())
}
