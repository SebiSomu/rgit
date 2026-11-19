//! Command-line interface of `rgit pack-refs`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit pack-refs`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/pack_refs/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_pack_refs`
#[derive(clap::Args, Debug)]
pub struct PackRefsArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit pack-refs` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_pack_refs(args: PackRefsArgs) -> anyhow::Result<()> {
    let PackRefsArgs { args } = args;
    crate::references::pack_refs::pack_refs(crate::cli::raw_args_after("pack-refs", args))?;
    Ok(())
}
