//! Types of `rgit open`.

/// The kind of web host a remote lives on, which decides its URL layout.
// Used by: src/remote/open/types.rs, src/remote/open/engine.rs, src/remote/open.rs
// Used by command/helper: `web_repo`, `url_for` and the `open` command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    GitHub,
    GitLab,
    Bitbucket,
}

/// A remote's web page: the host kind and the repository's base URL
/// (`https://github.com/owner/repo`).
// Used by: src/remote/open/types.rs, src/remote/open/engine.rs, src/remote/open.rs
// Used by command/helper: `web_repo`, `url_for` and the `open` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebRepo {
    pub host: Host,
    pub base: String,
}

/// What `rgit open` shows: the repository at a ref, a file or directory (with an optional line
/// range) at a ref, a commit, a new pull request from a branch into a base, or an existing one.
// Used by: src/remote/open/types.rs, src/remote/open/engine.rs, src/remote/open.rs
// Used by command/helper: `url_for` and the `open` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Tree { reference: String },
    Path { reference: String, path: String, dir: bool, lines: Option<(usize, usize)> },
    Commit(String),
    NewPullRequest { branch: String, base: String },
    PullRequest(u64),
}
