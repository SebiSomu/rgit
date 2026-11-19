//! Command-line interface of `rgit ls-files`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit ls-files`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/ls_files/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_ls_files`
#[derive(clap::Args, Debug)]
pub struct LsFilesArgs {
    #[arg(short = 'c', long = "cached")]
    cached: bool,
    #[arg(short = 'd', long = "deleted")]
    deleted: bool,
    #[arg(short = 'm', long = "modified")]
    modified: bool,
    #[arg(short = 'o', long = "others")]
    others: bool,
    #[arg(short = 'i', long = "ignored")]
    ignored: bool,
    #[arg(long = "exclude-standard")]
    exclude_standard: bool,
    #[arg(short = 's', long = "stage")]
    stage: bool,
    #[arg(short = 'u', long = "unmerged")]
    unmerged: bool,
    #[arg(short = 't')]
    show_tags: bool,
    #[arg(short = 'z')]
    null_terminated: bool,
    #[arg(value_name = "PATHSPEC")]
    pathspecs: Vec<String>,
}

/// Runs `rgit ls-files` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_ls_files(args: LsFilesArgs) -> anyhow::Result<()> {
    let LsFilesArgs {
        cached,
        deleted,
        modified,
        others,
        ignored,
        exclude_standard,
        stage,
        unmerged,
        show_tags,
        null_terminated,
        pathspecs,
    } = args;
    let opts = crate::workspace::ls_files::types::LsFilesOptions {
        cached,
        deleted,
        modified,
        others,
        ignored,
        exclude_standard,
        // Unmerged entries only make sense with their stage numbers.
        stage: stage || unmerged,
        unmerged,
        show_tags,
        null_terminated,
        pathspecs,
    };
    crate::workspace::ls_files::ls_files(opts)?;
    Ok(())
}
