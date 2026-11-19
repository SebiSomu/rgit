//! Command-line interface of `rgit pack-objects`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit pack-objects`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/storage/pack_objects/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_pack_objects`
#[derive(clap::Args, Debug)]
pub struct PackObjectsArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit pack-objects` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_pack_objects(args: PackObjectsArgs) -> anyhow::Result<()> {
    let PackObjectsArgs { args } = args;
    crate::storage::pack_objects::pack_objects(crate::cli::raw_args_after("pack-objects", args))?;
    Ok(())
}
