//! Command-line interface of `rgit hash-object`, `rgit cat-file`, `rgit ls-tree`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

use std::path::PathBuf;

// Command-line arguments of `rgit hash-object`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/basic/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_hash_object`
#[derive(clap::Args, Debug)]
pub struct HashObjectArgs {
    #[arg(short = 'w')]
    write: bool,
    file: PathBuf
}

/// Runs `rgit hash-object` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_hash_object(args: HashObjectArgs) -> anyhow::Result<()> {
    let HashObjectArgs { write, file} = args;
    crate::plumbing::basic::hash_object(write, file)?;
    Ok(())
}

// Command-line arguments of `rgit cat-file`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/basic/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_cat_file`
#[derive(clap::Args, Debug)]
pub struct CatFileArgs {
    #[arg(short = 'p')]
    pretty_print: bool,
    object_hash: String
}

/// Runs `rgit cat-file` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_cat_file(args: CatFileArgs) -> anyhow::Result<()> {
    let CatFileArgs { pretty_print, object_hash } = args;
    crate::plumbing::basic::cat_file(pretty_print, object_hash)?;
    Ok(())
}

// Command-line arguments of `rgit ls-tree`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/plumbing/basic/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_ls_tree`
#[derive(clap::Args, Debug)]
pub struct LsTreeArgs {
    #[arg(long)]
    name_only: bool,
    tree_hash: String
}

/// Runs `rgit ls-tree` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_ls_tree(args: LsTreeArgs) -> anyhow::Result<()> {
    let LsTreeArgs { name_only, tree_hash } = args;
    crate::plumbing::basic::ls_tree(name_only, tree_hash)?;
    Ok(())
}
