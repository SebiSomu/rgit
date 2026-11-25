//! Command-line interface of `rgit open`: the clap argument struct and the dispatch function
//! that `cli.rs` calls.

// Command-line arguments of `rgit open`, parsed by clap through `cli::Commands` (a `///` doc
// comment here would become the command's help text).
// Used by: src/cli.rs, src/remote/open/cli.rs
// Used by command/helper: `cli::handle_commands` and `dispatch_open`
#[derive(clap::Args, Debug)]
pub struct OpenArgs {
    /// A file or directory (`path:12` or `path:12-20` for lines), a branch, a commit, `pr` (a new pull request from the current branch) or `#123` (pull request 123). Default: the current branch.
    #[arg(value_name = "TARGET")]
    target: Option<String>,
    /// Only print the URL, don't open the browser.
    #[arg(short = 'p', long = "print")]
    print: bool,
    /// The remote whose web page to use (default: origin, or the only remote).
    #[arg(long = "remote", value_name = "NAME")]
    remote: Option<String>,
}

/// Runs `rgit open` with its parsed arguments.
// Used by `cli::handle_commands`.
pub fn dispatch_open(args: OpenArgs) -> anyhow::Result<()> {
    if !crate::core::repo::exists() {
        anyhow::bail!("fatal: not a git repository (run rgit from the top of the working tree)");
    }
    crate::remote::open::open(args.target, args.print, args.remote)
}
