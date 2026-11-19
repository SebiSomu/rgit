/// Target repository metadata and authentication credentials for pushing to GitHub via the Git Data API.
// Used by: src/mail/request_pull/engine.rs, src/remote/clone/engine.rs, src/remote/fetch.rs, src/remote/github.rs, src/remote/push.rs
// Used by command/helper: `push_to_github`, `resolve_github_target`, and the `push` command
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitHubTarget {
    pub owner: String,
    pub repo: String,
    pub token: String,
}
