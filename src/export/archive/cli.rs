//! Command-line interface of `rgit archive`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

use std::path::PathBuf;

// Command-line arguments of `rgit archive`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/export/archive/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_archive`
#[derive(clap::Args, Debug)]
pub struct ArchiveArgs {
    #[arg(short = 'f', long = "format")]
    format: Option<String>,
    #[arg(short = 'l', long = "list")]
    list: bool,
    #[arg(long = "prefix")]
    prefix: Option<String>,
    #[arg(short = 'o', long = "output")]
    output: Option<PathBuf>,
    #[arg(value_name = "TREE-ISH")]
    tree_ish: Option<String>,
    #[arg(value_name = "PATH")]
    paths: Vec<String>,
}

/// Runs `rgit archive` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_archive(args: ArchiveArgs) -> anyhow::Result<()> {
    let ArchiveArgs { format, list, prefix, output, tree_ish, paths } = args;
    crate::export::archive::archive(format, list, prefix, output, tree_ish, paths)?;
    Ok(())
}
