//! Picking the transport for a remote: one place that turns a remote name, path or URL into
//! a `Transport`, so `fetch`, `push`, `clone` and `request-pull` agree on how to reach it.

pub mod types;

use std::path::Path;
use anyhow::Result;

use crate::remote::bundle::engine::is_bundle_file;
use crate::remote::github::parse_github_remote;
use crate::remote::push::engine::resolve_remote_repo_path;
use crate::remote::transport::types::{Remote, Transport};

/// Resolves a remote given as a configured name (`origin`), a path or a URL. The URL is
/// `remote.<spec>.url` when configured, else `spec` itself. A path that exists on disk is a
/// bundle (by its signature) or a repository; anything else GitHub recognizes is a GitHub
/// repository; any other `http(s)://` URL is smart HTTP; the rest fails with the usual
/// "does not appear to be a git repository" errors.
// Used by `fetch`, `push`, `clone` and `request-pull`.
pub fn resolve_remote(spec: &str) -> Result<Remote> {
    let configured_url = crate::core::config::get_config_value(&format!("remote.{}.url", spec))?;
    let url = configured_url.clone().unwrap_or_else(|| spec.to_string());
    let path = Path::new(&url);

    let transport = if path.exists() && is_bundle_file(path) {
        Transport::Bundle { path: path.to_path_buf() }
    } else if path.exists() {
        Transport::Local { git_dir: resolve_remote_repo_path(spec)? }
    } else if let Some((owner, repo)) = parse_github_remote(&url) {
        if github_uses_smart_http() {
            Transport::SmartHttp { url: github_https_url(&url, &owner, &repo) }
        } else {
            Transport::GitHubApi { owner, repo }
        }
    } else if url.starts_with("https://") || url.starts_with("http://") {
        Transport::SmartHttp { url }
    } else {
        // Not on disk and not GitHub: this reports the missing path the way it always has.
        Transport::Local { git_dir: resolve_remote_repo_path(spec)? }
    };

    Ok(Remote { spec: spec.to_string(), configured_url, transport })
}

/// The error a command gives when it can only work with repositories and got a bundle
/// (`push` to a bundle, `request-pull` against one): the same as for any other file.
// Used by `push` and `request-pull`.
pub fn not_a_repository(path: &Path) -> anyhow::Error {
    anyhow::anyhow!("fatal: '{}' does not appear to be a git repository", path.display())
}

/// The error for a command that can't use git's smart-HTTP protocol yet (`fetch`, `push`,
/// `request-pull` to a host other than GitHub), pointing at what does work.
// Used by `fetch`, `push` and `request-pull`.
pub fn smart_http_not_supported_yet(action: &str, url: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "fatal: rgit can't {} '{}' yet: over HTTPS only GitHub is supported so far\nhint: `rgit ls-remote {}` already works with any host",
        action,
        crate::remote::http::redact_url(url),
        crate::remote::http::redact_url(url)
    )
}

/// Whether GitHub remotes use git's smart-HTTP protocol instead of the REST API: opt in with
/// `rgit config --global rgit.httpTransport smart` (until smart HTTP becomes the default).
// Used by `resolve_remote` and `clone`.
pub fn github_uses_smart_http() -> bool {
    crate::core::config::get_config_value("rgit.httptransport")
        .ok()
        .flatten()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("smart"))
}

/// The https URL of a GitHub repository: the URL itself when it already is one, else
/// `https://github.com/<owner>/<repo>.git` (for `git@github.com:` and shorthand remotes).
// Used by `resolve_remote`, `clone` and `ls-remote`.
pub fn github_https_url(url: &str, owner: &str, repo: &str) -> String {
    if url.starts_with("https://") || url.starts_with("http://") {
        url.to_string()
    } else {
        format!("https://github.com/{}/{}.git", owner, repo)
    }
}
