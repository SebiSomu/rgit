//! Command-line interface of `rgit tag`, `rgit describe`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

// Command-line arguments of `rgit tag`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/tag/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_tag`
#[derive(clap::Args, Debug)]
pub struct TagArgs {
    #[arg(short = 'l', long = "list")]
    list: bool,
    #[arg(short = 'd', long = "delete")]
    delete: bool,
    #[arg(short = 'a', long = "annotate")]
    annotate: bool,
    #[arg(short = 'f', long = "force")]
    force: bool,
    #[arg(short = 'm', long = "message", value_name = "MSG")]
    messages: Vec<String>,
    #[arg(short = 'F', long = "file", value_name = "FILE")]
    file: Option<String>,
    #[arg(short = 'e', long = "edit")]
    edit: bool,
    /// -n[<num>]: lines of each tag message to print (attached number, like git)
    #[arg(short = 'n', num_args = 0..=1, default_missing_value = "1", value_name = "NUM")]
    lines: Option<String>,
    #[arg(long = "contains", value_name = "COMMIT")]
    contains: Option<String>,
    #[arg(long = "points-at", value_name = "OBJECT")]
    points_at: Option<String>,
    args: Vec<String>,
}

/// Runs `rgit tag` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_tag(args: TagArgs) -> anyhow::Result<()> {
    let TagArgs { list, delete, annotate, force, messages, file, edit, lines, contains, points_at, mut args } = args;
    // git's `-n` only takes an attached number (`-n2`); a separate word after it is a pattern.
    let lines = lines.map(|value| match value.parse::<usize>() {
        Ok(n) => n,
        Err(_) => {
            args.insert(0, value);
            1
        }
    });
    let opts = crate::references::tag::types::TagOptions { list, delete, annotate, force, messages, file, edit, lines, contains, points_at, args };
    crate::references::tag::tag(opts)?;
    Ok(())
}

// Command-line arguments of `rgit describe`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/references/tag/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_describe`
#[derive(clap::Args, Debug)]
pub struct DescribeArgs {
    #[arg(long = "tags")]
    tags: bool,
    #[arg(long = "always")]
    always: bool,
    #[arg(long = "long")]
    long: bool,
    #[arg(long = "abbrev", value_name = "N", require_equals = true)]
    abbrev: Option<usize>,
    #[arg(long = "exact-match")]
    exact_match: bool,
    commits: Vec<String>,
}

/// Runs `rgit describe` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_describe(args: DescribeArgs) -> anyhow::Result<()> {
    let DescribeArgs { tags, always, long, abbrev, exact_match, commits } = args;
    let opts = crate::references::tag::types::DescribeOptions { tags, always, long, abbrev, exact_match, commits };
    crate::references::tag::describe(opts)?;
    Ok(())
}
