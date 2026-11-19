/// Parsed arguments for the `request-pull` command, which writes a summary of pending
/// changes for someone to pull.
// Used by: src/mail/request_pull.rs, src/mail/request_pull/engine.rs
// Used by command/helper: `parse_request_pull_args` and the `request_pull` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestPullOptions {
    /// `-p`: include the full patch after the diffstat.
    pub patch: bool,
    /// `<start>`: the commit the upstream already has.
    pub start: String,
    /// `<url>`: the repository to be pulled from (a path, a URL, or a remote name).
    pub url: String,
    /// `<end>`: the commit to pull, optionally `<local>:<remote>`; empty means `HEAD`.
    pub end: String,
}
