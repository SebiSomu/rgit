//! Command-line interface of `rgit am`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit am`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/mail/am/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_am`
#[derive(clap::Args, Debug)]
pub struct AmArgs {
    #[arg(long = "continue", short = 'r', alias = "resolved", conflicts_with_all = ["skip", "abort", "quit", "show_current_patch"])]
    cont: bool,
    #[arg(long = "skip", conflicts_with_all = ["abort", "quit", "show_current_patch"])]
    skip: bool,
    #[arg(long = "abort", conflicts_with_all = ["quit", "show_current_patch"])]
    abort: bool,
    #[arg(long = "quit", conflicts_with = "show_current_patch")]
    quit: bool,
    #[arg(long = "show-current-patch", num_args = 0..=1, default_missing_value = "raw", require_equals = true, value_name = "raw|diff")]
    show_current_patch: Option<String>,
    #[arg(short = 'k', long = "keep-subject")]
    keep_subject: bool,
    #[arg(value_name = "MBOX")]
    mboxes: Vec<String>,
}

/// Runs `rgit am` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_am(args: AmArgs) -> anyhow::Result<()> {
    let AmArgs {
        cont,
        skip,
        abort,
        quit,
        show_current_patch,
        keep_subject,
        mboxes,
    } = args;
    let opts = crate::mail::am::types::AmOptions {
        mboxes,
        cont,
        skip,
        abort,
        quit,
        show_current_patch,
        keep_subject,
    };
    crate::mail::am::am(opts)?;
    Ok(())
}
