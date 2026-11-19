//! Command-line interface of `rgit shortlog`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit shortlog`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/shortlog/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_shortlog`
#[derive(clap::Args, Debug)]
pub struct ShortlogArgs {
    #[arg(short = 's', long = "summary")]
    summary: bool,
    #[arg(short = 'n', long = "numbered")]
    numbered: bool,
    #[arg(short = 'e', long = "email")]
    email: bool,
    #[arg(long = "all")]
    all: bool,
    rev: Option<String>,
}

/// Runs `rgit shortlog` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_shortlog(args: ShortlogArgs) -> anyhow::Result<()> {
    let ShortlogArgs {
        summary,
        numbered,
        email,
        all,
        rev,
    } = args;
    let opts = crate::history::shortlog::types::ShortlogOptions {
        summary,
        numbered,
        email,
        all,
        rev,
    };
    crate::history::shortlog::shortlog(opts)?;
    Ok(())
}
