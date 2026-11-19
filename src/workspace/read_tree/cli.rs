//! Command-line interface of `rgit read-tree`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit read-tree`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/read_tree/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_read_tree`
#[derive(clap::Args, Debug)]
pub struct ReadTreeArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit read-tree` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_read_tree(args: ReadTreeArgs) -> anyhow::Result<()> {
    let ReadTreeArgs { args } = args;
    crate::workspace::read_tree::read_tree(crate::cli::raw_args_after("read-tree", args))?;
    Ok(())
}
