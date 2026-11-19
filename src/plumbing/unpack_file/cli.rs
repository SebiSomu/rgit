//! Command-line interface of `rgit unpack-file`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit unpack-file`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/unpack_file/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_unpack_file`
#[derive(clap::Args, Debug)]
pub struct UnpackFileArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit unpack-file` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_unpack_file(args: UnpackFileArgs) -> anyhow::Result<()> {
    let UnpackFileArgs { args } = args;
    crate::plumbing::unpack_file::unpack_file(crate::cli::raw_args_after("unpack-file", args))?;
    Ok(())
}
