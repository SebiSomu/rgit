//! Command-line interface of `rgit patch-id`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit patch-id`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/diff/patch_id/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_patch_id`
#[derive(clap::Args, Debug)]
pub struct PatchIdArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit patch-id` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_patch_id(args: PatchIdArgs) -> anyhow::Result<()> {
    let PatchIdArgs { args } = args;
    crate::diff::patch_id::patch_id(crate::cli::raw_args_after("patch-id", args))?;
    Ok(())
}
