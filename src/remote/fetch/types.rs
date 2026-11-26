/// The update status of a single remote-tracking ref after a fetch operation.
// Used by: src/remote/fetch.rs, src/remote/github.rs
// Used by command/helper: `fetch_from_local`, `fetch_from_github`, and the `fetch` command
#[derive(Debug, PartialEq, Eq)]
pub enum FetchRefResult {
    NewBranch { branch: String, sha: String },
    Updated { branch: String, old_sha: String, new_sha: String },
    UpToDate { branch: String },
}

/// Which tags a fetch brings: by default the ones that point at something the repository
/// has after the fetch (git's auto-follow), with `--tags` all of them, with `--no-tags` none.
/// Only fetches over HTTPS act on it so far.
// Used by: src/remote/fetch.rs, src/remote/fetch/cli.rs
// Used by command/helper: `fetch` (`--tags`, `--no-tags`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TagMode {
    #[default]
    Follow,
    All,
    None,
}
