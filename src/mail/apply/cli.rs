//! Command-line interface of `rgit apply`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit apply`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/mail/apply/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_apply`
#[derive(clap::Args, Debug)]
pub struct ApplyArgs {
    #[arg(long = "check")]
    check: bool,
    #[arg(long = "stat")]
    stat: bool,
    #[arg(long = "numstat")]
    numstat: bool,
    #[arg(long = "summary")]
    summary: bool,
    #[arg(long = "apply")]
    apply: bool,
    #[arg(long = "cached")]
    cached: bool,
    #[arg(long = "index")]
    index: bool,
    #[arg(short = 'R', long = "reverse")]
    reverse: bool,
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
    #[arg(short = 'p', value_name = "N", default_value = "1")]
    strip: usize,
    #[arg(value_name = "PATCH")]
    patches: Vec<String>,
}

/// Runs `rgit apply` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_apply(args: ApplyArgs) -> anyhow::Result<()> {
    let ApplyArgs {
        check,
        stat,
        numstat,
        summary,
        apply,
        cached,
        index,
        reverse,
        verbose,
        strip,
        patches,
    } = args;
    let opts = crate::mail::apply::types::ApplyOptions {
        check,
        stat,
        numstat,
        summary,
        apply,
        cached,
        index,
        reverse,
        verbose,
        strip,
        patches,
    };
    crate::mail::apply::apply(opts)?;
    Ok(())
}
