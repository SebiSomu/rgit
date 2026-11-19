//! Command-line interface of `rgit push`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit push`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/push/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_push`
#[derive(clap::Args, Debug)]
pub struct PushArgs {
    remote: Option<String>,
    refspec: Option<String>,
    #[arg(short = 'f', long = "force")]
    force: bool,
    #[arg(short = 'u', long = "set-upstream")]
    set_upstream: bool,
}

/// Runs `rgit push` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_push(args: PushArgs) -> anyhow::Result<()> {
    let PushArgs { remote, refspec, force, set_upstream } = args;
    crate::remote::push::push(remote, refspec, force, set_upstream)?;
    Ok(())
}
