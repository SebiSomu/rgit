//! Command-line interface of `rgit fetch`: the clap argument struct and the dispatch
//! function that `cli.rs` calls.

// Command-line arguments of `rgit fetch`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/fetch/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_fetch`
#[derive(clap::Args, Debug)]
pub struct FetchArgs {
    remote: Option<String>,
    branch: Option<String>,
    /// Fetch all tags from the remote (over HTTPS)
    #[arg(short = 't', long, conflicts_with = "no_tags")]
    tags: bool,
    /// Do not fetch any tags
    #[arg(short = 'n', long = "no-tags")]
    no_tags: bool,
}

/// Runs `rgit fetch` with its parsed arguments (the dispatch that used to live in `cli.rs`).
// Used by `cli::handle_commands`.
pub fn dispatch_fetch(args: FetchArgs) -> anyhow::Result<()> {
    let FetchArgs { remote, branch, tags, no_tags } = args;
    let tag_mode = match (tags, no_tags) {
        (true, _) => crate::remote::fetch::types::TagMode::All,
        (_, true) => crate::remote::fetch::types::TagMode::None,
        _ => crate::remote::fetch::types::TagMode::Follow,
    };
    crate::remote::fetch::fetch_with_tags(remote, branch, tag_mode)?;
    Ok(())
}
