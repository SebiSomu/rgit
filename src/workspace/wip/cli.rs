//! Command-line interface of `rgit wip` (alias `rgit save`): the clap argument struct and the
//! dispatch function that `cli.rs` calls.

// Command-line arguments of `rgit wip`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/wip/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_wip`
#[derive(clap::Args, Debug)]
pub struct WipArgs {
    /// What you were in the middle of (the words are joined; no quotes needed).
    #[arg(value_name = "MESSAGE", conflicts_with_all = ["list", "restore", "drop"])]
    message: Vec<String>,
    /// List this branch's WIP saves, newest first.
    #[arg(short = 'l', long = "list", conflicts_with_all = ["restore", "drop"])]
    list: bool,
    /// With --list: every branch's saves.
    #[arg(long = "all", requires = "list")]
    all: bool,
    /// Bring a save back (default: the newest, wip@{0}) and take it off the stack.
    #[arg(short = 'r', long = "restore", value_name = "N", num_args = 0..=1, default_missing_value = "0", conflicts_with = "drop")]
    restore: Option<String>,
    /// Throw a save away (default: the newest).
    #[arg(long = "drop", value_name = "N", num_args = 0..=1, default_missing_value = "0")]
    drop: Option<String>,
}

/// Runs `rgit wip` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_wip(args: WipArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    if args.list {
        crate::workspace::wip::list(args.all)
    } else if let Some(n) = args.restore {
        crate::workspace::wip::restore(&n)
    } else if let Some(n) = args.drop {
        crate::workspace::wip::drop(&n)
    } else {
        let message = if args.message.is_empty() { None } else { Some(args.message.join(" ")) };
        crate::workspace::wip::save(message)
    }
}
