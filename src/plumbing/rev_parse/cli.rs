//! Command-line interface of `rgit rev-parse`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit rev-parse`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/rev_parse/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_rev_parse`
#[derive(clap::Args, Debug)]
pub struct RevParseArgs {
    #[arg(long = "git-dir")]
    git_dir: bool,
    #[arg(long = "show-toplevel")]
    show_toplevel: bool,
    #[arg(long = "is-inside-work-tree")]
    is_inside_work_tree: bool,
    #[arg(long = "is-bare-repository")]
    is_bare_repository: bool,
    #[arg(long = "abbrev-ref")]
    abbrev_ref: bool,
    #[arg(long = "symbolic-full-name")]
    symbolic_full_name: bool,
    #[arg(long = "verify")]
    verify: bool,
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
    #[arg(long = "short", num_args = 0..=1, default_missing_value = "7", require_equals = true, value_name = "N")]
    short: Option<usize>,
    #[arg(long = "sq")]
    sq: bool,
    revs: Vec<String>,
}

/// Runs `rgit rev-parse` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_rev_parse(args: RevParseArgs) -> anyhow::Result<()> {
    let RevParseArgs {
        git_dir,
        show_toplevel,
        is_inside_work_tree,
        is_bare_repository,
        abbrev_ref,
        symbolic_full_name,
        verify,
        quiet,
        short,
        sq,
        revs,
    } = args;
    let opts = crate::plumbing::rev_parse::types::RevParseOptions {
        git_dir,
        show_toplevel,
        is_inside_work_tree,
        is_bare_repository,
        abbrev_ref,
        symbolic_full_name,
        verify,
        quiet,
        short,
        sq,
        revs,
    };
    crate::plumbing::rev_parse::rev_parse(opts)?;
    Ok(())
}
