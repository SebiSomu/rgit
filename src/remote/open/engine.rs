//! Helpers of `rgit open`: turning a remote URL into its web page, building the URL of a file,
//! commit, branch or pull request for GitHub, GitLab and Bitbucket, and opening a browser.

use anyhow::Result;

use crate::remote::open::types::{Host, Target, WebRepo};

/// The web page of a remote URL: `https://host/path`, `ssh://git@host[:port]/path`,
/// `git@host:path` or rgit's `owner/repo` GitHub shorthand. None for local paths and hosts
/// whose web layout isn't known.
// Used by the `open` command.
pub fn web_repo(url: &str) -> Option<WebRepo> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) {
        let rest = rest.rsplit_once('@').map(|(_, r)| r).unwrap_or(rest);
        rest.split_once('/')?
    } else if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = rest.rsplit_once('@').map(|(_, r)| r).unwrap_or(rest);
        let (host, path) = rest.split_once('/')?;
        (host.split(':').next().unwrap_or(host), path)
    } else if let Some((user_host, path)) = url.split_once(':').filter(|(h, _)| h.contains('@') && !h.contains('/')) {
        (user_host.rsplit_once('@').map(|(_, h)| h).unwrap_or(user_host), path)
    } else if url.matches('/').count() == 1 && !url.starts_with('.') && !url.contains(':') && !std::path::Path::new(url).exists() {
        ("github.com", url)
    } else {
        return None;
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    if path.is_empty() {
        return None;
    }
    let lower = host.to_ascii_lowercase();
    let kind = if lower.contains("github") {
        Host::GitHub
    } else if lower.contains("gitlab") {
        Host::GitLab
    } else if lower.contains("bitbucket") {
        Host::Bitbucket
    } else {
        return None;
    };
    Some(WebRepo { host: kind, base: format!("https://{}/{}", host, path) })
}

/// Percent-encodes what a URL path can't hold as is (spaces, `#`, `?`, `%`, non-ASCII).
// Used by `url_for`.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// The web URL of `target` on `repo`.
// Used by the `open` command.
pub fn url_for(repo: &WebRepo, target: &Target) -> String {
    let b = &repo.base;
    match (repo.host, target) {
        (Host::GitHub, Target::Tree { reference }) => format!("{}/tree/{}", b, encode(reference)),
        (Host::GitLab, Target::Tree { reference }) => format!("{}/-/tree/{}", b, encode(reference)),
        (Host::Bitbucket, Target::Tree { reference }) => format!("{}/src/{}", b, encode(reference)),
        (host, Target::Path { reference, path, dir, lines }) => {
            let anchor = match (host, lines) {
                (_, None) => String::new(),
                (Host::GitHub, Some((s, e))) if s == e => format!("#L{}", s),
                (Host::GitHub, Some((s, e))) => format!("#L{}-L{}", s, e),
                (Host::GitLab, Some((s, e))) if s == e => format!("#L{}", s),
                (Host::GitLab, Some((s, e))) => format!("#L{}-{}", s, e),
                (Host::Bitbucket, Some((s, e))) if s == e => format!("#lines-{}", s),
                (Host::Bitbucket, Some((s, e))) => format!("#lines-{}:{}", s, e),
            };
            let kind = if *dir { "tree" } else { "blob" };
            match host {
                Host::GitHub => format!("{}/{}/{}/{}{}", b, kind, encode(reference), encode(path), anchor),
                Host::GitLab => format!("{}/-/{}/{}/{}{}", b, kind, encode(reference), encode(path), anchor),
                Host::Bitbucket => format!("{}/src/{}/{}{}", b, encode(reference), encode(path), anchor),
            }
        }
        (Host::GitHub, Target::Commit(h)) => format!("{}/commit/{}", b, h),
        (Host::GitLab, Target::Commit(h)) => format!("{}/-/commit/{}", b, h),
        (Host::Bitbucket, Target::Commit(h)) => format!("{}/commits/{}", b, h),
        (Host::GitHub, Target::NewPullRequest { branch, base }) => format!("{}/compare/{}...{}?expand=1", b, encode(base), encode(branch)),
        (Host::GitLab, Target::NewPullRequest { branch, base }) => {
            format!("{}/-/merge_requests/new?merge_request%5Bsource_branch%5D={}&merge_request%5Btarget_branch%5D={}", b, encode(branch), encode(base))
        }
        (Host::Bitbucket, Target::NewPullRequest { branch, base }) => format!("{}/pull-requests/new?source={}&dest={}", b, encode(branch), encode(base)),
        (Host::GitHub, Target::PullRequest(n)) => format!("{}/pull/{}", b, n),
        (Host::GitLab, Target::PullRequest(n)) => format!("{}/-/merge_requests/{}", b, n),
        (Host::Bitbucket, Target::PullRequest(n)) => format!("{}/pull-requests/{}", b, n),
    }
}

/// `path:12` / `path:12-20` split into the path and its line range.
// Used by the `open` command.
pub fn split_lines(spec: &str) -> (String, Option<(usize, usize)>) {
    if let Some((path, range)) = spec.rsplit_once(':') {
        let parsed = match range.split_once('-') {
            Some((s, e)) => s.parse().ok().zip(e.parse().ok()),
            None => range.parse().ok().map(|n| (n, n)),
        };
        if let Some((s, e)) = parsed.filter(|(s, e): &(usize, usize)| *s >= 1 && e >= s) {
            return (path.to_string(), Some((s, e)));
        }
    }
    (spec.to_string(), None)
}

/// Opens `url` in the default browser (`start` on Windows, `open` on macOS, `xdg-open`
/// elsewhere).
// Used by the `open` command.
pub fn launch(url: &str) -> Result<()> {
    let status = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", url]).status()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()
    } else {
        std::process::Command::new("xdg-open").arg(url).status()
    };
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => anyhow::bail!("couldn't start a browser"),
    }
}
