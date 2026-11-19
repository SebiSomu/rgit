//! Command-line interface of `rgit commit-tree`, `rgit commit`: the clap argument structs and the dispatch
//! functions that `cli.rs` calls.

// Command-line arguments of `rgit commit-tree`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/commit/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_commit_tree`
#[derive(clap::Args, Debug)]
pub struct CommitTreeArgs {
    tree_hash: String,
    #[arg(short = 'p')]
    parent_hash: Option<String>,
    #[arg(short = 'm')]
    message: String
}

/// Runs `rgit commit-tree` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_commit_tree(args: CommitTreeArgs) -> anyhow::Result<()> {
    let CommitTreeArgs { tree_hash, parent_hash, message } = args;
    crate::workspace::commit::commit_tree(tree_hash, parent_hash, message)?;
    Ok(())
}

// Command-line arguments of `rgit commit`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/workspace/commit/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_commit`
#[derive(clap::Args, Debug)]
pub struct CommitArgs {
    #[arg(short = 'm', long = "message", value_name = "MSG")]
    messages: Vec<String>,
    #[arg(short = 'F', long = "file", value_name = "FILE", conflicts_with = "messages")]
    file: Option<String>,
    #[arg(short = 'C', long = "reuse-message", value_name = "COMMIT", conflicts_with_all = ["messages", "file", "reedit_message"])]
    reuse_message: Option<String>,
    #[arg(short = 'c', long = "reedit-message", value_name = "COMMIT", conflicts_with_all = ["messages", "file"])]
    reedit_message: Option<String>,
    #[arg(short = 'a', long = "all")]
    all: bool,
    #[arg(long = "amend")]
    amend: bool,
    #[arg(short = 'e', long = "edit", conflicts_with = "no_edit")]
    edit: bool,
    #[arg(long = "no-edit")]
    no_edit: bool,
    #[arg(long = "author", value_name = "AUTHOR")]
    author: Option<String>,
    #[arg(long = "date", value_name = "DATE")]
    date: Option<String>,
    #[arg(long = "reset-author")]
    reset_author: bool,
    #[arg(long = "allow-empty")]
    allow_empty: bool,
    #[arg(long = "allow-empty-message")]
    allow_empty_message: bool,
    #[arg(short = 's', long = "signoff")]
    signoff: bool,
    /// `strip` (drop `#` lines), `whitespace` or `default`.
    #[arg(long = "cleanup", value_name = "MODE", value_parser = ["strip", "whitespace", "default"])]
    cleanup: Option<String>,
    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
}

/// Runs `rgit commit` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_commit(args: CommitArgs) -> anyhow::Result<()> {
    let CommitArgs {
        messages,
        file,
        reuse_message,
        reedit_message,
        all,
        amend,
        edit,
        no_edit,
        author,
        date,
        reset_author,
        allow_empty,
        allow_empty_message,
        signoff,
        cleanup,
        quiet,
    } = args;
    let opts = crate::workspace::commit::types::CommitOptions {
        messages,
        file,
        reedit: reedit_message.is_some(),
        reuse_message: reuse_message.or(reedit_message),
        all,
        amend,
        edit,
        no_edit,
        author,
        date,
        reset_author,
        allow_empty,
        allow_empty_message,
        signoff,
        cleanup_strip: cleanup.as_deref() == Some("strip"),
        quiet,
    };
    crate::workspace::commit::commit(opts)?;
    Ok(())
}
