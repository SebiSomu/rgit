use std::path::PathBuf;

/// How rgit reaches a remote repository, decided once from its URL by `resolve_remote`:
/// a repository on this machine (objects copied directly), a bundle file, or a GitHub
/// repository through the Git Data API. Smart HTTP and SSH join this list later.
// Used by: src/remote/transport.rs, src/remote/fetch.rs, src/remote/push.rs, src/remote/clone/engine.rs, src/mail/request_pull/engine.rs
// Used by command/helper: `resolve_remote` and the `fetch`, `push`, `clone` and `request-pull` commands
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    /// A work tree's `.git` directory or a bare repository on this machine.
    Local { git_dir: PathBuf },
    /// A file written by `rgit bundle create` (or `git bundle create`).
    Bundle { path: PathBuf },
    /// A GitHub repository, reached through its REST API.
    GitHubApi { owner: String, repo: String },
    /// Any other `http://` or `https://` URL: git's smart-HTTP protocol (`ls-remote` so far).
    SmartHttp { url: String },
}

/// A remote as a command sees it: the name or URL it was given, the URL configured for that
/// name (`remote.<name>.url`), and how to reach it.
// Used by: src/remote/transport.rs, src/remote/fetch.rs, src/remote/push.rs, src/remote/clone/engine.rs, src/mail/request_pull/engine.rs
// Used by command/helper: `resolve_remote` and the `fetch`, `push`, `clone` and `request-pull` commands
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub spec: String,
    pub configured_url: Option<String>,
    pub transport: Transport,
}

impl Remote {
    /// The URL shown in `From <url>` / `To <url>` lines: the configured URL, else one made
    /// from the transport (the GitHub URL, the bundle as given, the repository's directory).
    pub fn display(&self) -> String {
        if let Some(url) = &self.configured_url {
            return url.clone();
        }
        match &self.transport {
            Transport::GitHubApi { owner, repo } => format!("https://github.com/{}/{}", owner, repo),
            // A path is shown as it was given, as git does (`From ../lib`), not as the `.git`
            // directory found inside it (which would also mix `/` and `\` on Windows).
            Transport::Bundle { .. } | Transport::Local { .. } => self.spec.clone(),
            Transport::SmartHttp { url } => url.clone(),
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/remote/transport/types.rs"]
mod tests;
