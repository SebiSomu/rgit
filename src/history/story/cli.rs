//! Command-line interface of `rgit story`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit story`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/history/story/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_story`
#[derive(clap::Args, Debug)]
pub struct StoryArgs {
    /// The file or directory.
    #[arg(value_name = "PATH")]
    path: String,
    /// How many commits the timeline lists (newest first).
    #[arg(short = 'n', long = "max-count", default_value_t = 10, value_name = "N")]
    max_count: usize,
    /// List every commit in the timeline.
    #[arg(long = "all", conflicts_with = "max_count")]
    all: bool,
}

/// Runs `rgit story` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_story(args: StoryArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::history::story::story(&args.path, if args.all { usize::MAX } else { args.max_count })
}
