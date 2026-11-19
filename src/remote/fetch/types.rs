/// The update status of a single remote-tracking ref after a fetch operation.
// Used by: src/remote/fetch.rs, src/remote/github.rs
// Used by command/helper: `fetch_from_local`, `fetch_from_github`, and the `fetch` command
#[derive(Debug, PartialEq, Eq)]
pub enum FetchRefResult {
    NewBranch { branch: String, sha: String },
    Updated { branch: String, old_sha: String, new_sha: String },
    UpToDate { branch: String },
}
