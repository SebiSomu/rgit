/// Parsed author or committer metadata extracted from a raw commit object for API serialization.
// Used by: src/remote/github.rs
// Used by command/helper: `parse_commit_identity`, `create_github_commit`
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommitIdentity {
    pub name: String,
    pub email: String,
    pub date: String,
}

/// A single file or subtree entry in a GitHub tree creation request.
// Used by: src/remote/github.rs
// Used by command/helper: `upload_tree_to_github`, `create_github_tree`
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GitHubTreeEntry {
    pub path: String,
    pub mode: String,
    pub r#type: String,
    pub sha: String,
}
