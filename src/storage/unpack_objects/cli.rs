//! Command-line interface of `rgit unpack-objects`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit unpack-objects`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/unpack_objects/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_unpack_objects`
#[derive(clap::Args, Debug)]
pub struct UnpackObjectsArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit unpack-objects` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_unpack_objects(args: UnpackObjectsArgs) -> anyhow::Result<()> {
    let UnpackObjectsArgs { args } = args;
    crate::storage::unpack_objects::unpack_objects(crate::cli::raw_args_after("unpack-objects", args))?;
    Ok(())
}
