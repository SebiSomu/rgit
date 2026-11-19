//! Command-line interface of `rgit fmt-merge-msg`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit fmt-merge-msg`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/fmt_merge_msg/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_fmt_merge_msg`
#[derive(clap::Args, Debug)]
pub struct FmtMergeMsgArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit fmt-merge-msg` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_fmt_merge_msg(args: FmtMergeMsgArgs) -> anyhow::Result<()> {
    let FmtMergeMsgArgs { args } = args;
    crate::merge::fmt_merge_msg::fmt_merge_msg(crate::cli::raw_args_after("fmt-merge-msg", args))?;
    Ok(())
}
