use std::path::{Path, PathBuf};

/// The place a `clone` reads from, once the repository argument has been classified. It is
/// either a directory on the local filesystem (cloned by copying objects across, the same
/// way `fetch` reads a local remote), a bundle file (`rgit bundle create`), or a GitHub
/// repository reached through the Git Data API.
// Used by: src/remote/clone/types.rs, src/remote/clone.rs, src/remote/clone/engine.rs
// Used by command/helper: `classify_clone_source`, `remote_default_branch`, and the `clone` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloneSource {
    Local { path: PathBuf, git_dir: PathBuf },
    GitHub { owner: String, repo: String },
    Bundle { path: PathBuf },
}

impl CloneSource {
    /// The string stored as `remote.<name>.url`: the absolute path for a local source or bundle, the
    /// https URL for a GitHub one.
    pub fn url(&self) -> String {
        match self {
            CloneSource::Local { path, .. } => path.display().to_string(),
            CloneSource::GitHub { owner, repo } => format!("https://github.com/{}/{}", owner, repo),
            CloneSource::Bundle { path } => path.display().to_string(),
        }
    }
 
    /// The directory a clone goes into when none is given: the repository's name, with any
    /// `.git` suffix dropped (`proj.git` -> `proj`, `proj/.git` -> `proj`) and, for a bundle, its
    /// `.bundle` suffix (`proj.bundle` -> `proj`).
    pub fn default_dir_name(&self) -> String {
        match self {
            CloneSource::Local { path, .. } => {
                let name_of = |p: &Path| {
                    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                };
                let mut name = name_of(path);
                if name == ".git" {
                    name = path.parent().map(name_of).unwrap_or_default();
                }
                let name = name.strip_suffix(".git").unwrap_or(&name).to_string();
                if name.is_empty() { "repository".to_string() } else { name }
            }
            CloneSource::GitHub { repo, .. } => repo.clone(),
            CloneSource::Bundle { path } => {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let name = name.strip_suffix(".bundle").unwrap_or(&name);
                name.strip_suffix(".git").unwrap_or(name).to_string()
            }
        }
    }
}
