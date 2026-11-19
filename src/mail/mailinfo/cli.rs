//! Command-line interface of `rgit mailinfo`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit mailinfo`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/mail/mailinfo/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_mailinfo`
#[derive(clap::Args, Debug)]
pub struct MailinfoArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0.., value_name = "ARGS")]
    args: Vec<String>,
}

/// Runs `rgit mailinfo` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_mailinfo(args: MailinfoArgs) -> anyhow::Result<()> {
    let MailinfoArgs { args } = args;
    crate::mail::mailinfo::mailinfo(crate::cli::raw_args_after("mailinfo", args))?;
    Ok(())
}
