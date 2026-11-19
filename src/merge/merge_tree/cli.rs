//! Command-line interface of `rgit merge-tree`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit merge-tree`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/merge/merge_tree/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_merge_tree`
#[derive(clap::Args, Debug)]
pub struct MergeTreeArgs {
    #[arg(long = "write-tree")]
    write_tree: bool,
    #[arg(long = "name-only")]
    name_only: bool,
    #[arg(long = "messages", overrides_with = "no_messages")]
    messages: bool,
    #[arg(long = "no-messages", overrides_with = "messages")]
    no_messages: bool,
    #[arg(short = 'z')]
    nul: bool,
    #[arg(long = "merge-base", value_name = "TREE-ISH")]
    merge_base: Option<String>,
    #[arg(long = "allow-unrelated-histories")]
    allow_unrelated_histories: bool,
    #[arg(long = "explain")]
    explain: bool,
    branch1: String,
    branch2: String,
}

/// Runs `rgit merge-tree` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_merge_tree(args: MergeTreeArgs) -> anyhow::Result<()> {
    let MergeTreeArgs {
        write_tree: _,
        name_only,
        messages,
        no_messages,
        nul,
        merge_base,
        allow_unrelated_histories,
        explain,
        branch1,
        branch2,
    } = args;
    let opts = crate::merge::merge_tree::types::MergeTreeOptions {
        name_only,
        messages: if messages { Some(true) } else if no_messages { Some(false) } else { None },
        nul,
        merge_base,
        allow_unrelated_histories,
        explain,
        branch1,
        branch2,
    };
    crate::merge::merge_tree::merge_tree(opts)?;
    Ok(())
}
