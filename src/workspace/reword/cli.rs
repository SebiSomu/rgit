//! Command-line interface of `rgit reword`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit reword`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/reword/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_reword`
#[derive(clap::Args, Debug)]
pub struct RewordArgs {
    /// The commit whose message to change (default: HEAD)
    commit: Option<String>,
    /// Use this message instead of opening the editor (several -m make paragraphs, as in commit)
    #[arg(short = 'm', long = "message")]
    message: Vec<String>,
    /// Reword even if the commit is already pushed
    #[arg(short = 'f', long = "force")]
    force: bool,
}

/// Runs `rgit reword` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_reword(args: RewordArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    let message = (!args.message.is_empty()).then(|| args.message.join("\n\n"));
    crate::workspace::reword::reword(args.commit, message, args.force)
}
