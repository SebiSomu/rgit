//! Command-line interface of `rgit mailsplit`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit mailsplit`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/mail/mailsplit/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_mailsplit`
#[derive(clap::Args, Debug)]
pub struct MailsplitArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit mailsplit` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_mailsplit(args: MailsplitArgs) -> anyhow::Result<()> {
    let MailsplitArgs { args } = args;
    crate::mail::mailsplit::mailsplit(crate::cli::raw_args_after("mailsplit", args))?;
    Ok(())
}
