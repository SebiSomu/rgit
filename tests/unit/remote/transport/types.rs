//! Unit tests for `src/remote/transport/types.rs`, compiled into that module via `#[path]`.

use super::*;

fn remote(spec: &str, configured_url: Option<&str>, transport: Transport) -> Remote {
    Remote { spec: spec.to_string(), configured_url: configured_url.map(str::to_string), transport }
}

#[test]
fn display_prefers_the_configured_url() {
    let github = Transport::GitHubApi { owner: "SebiSomu".into(), repo: "RGit".into() };
    assert_eq!(remote("origin", Some("git@github.com:SebiSomu/RGit.git"), github.clone()).display(), "git@github.com:SebiSomu/RGit.git");
    assert_eq!(remote("SebiSomu/RGit", None, github).display(), "https://github.com/SebiSomu/RGit");
}

#[test]
fn display_without_configuration_uses_what_was_given() {
    let bundle = Transport::Bundle { path: PathBuf::from("../full.bundle") };
    assert_eq!(remote("../full.bundle", None, bundle).display(), "../full.bundle");
    let local = Transport::Local { git_dir: PathBuf::from("/srv/repo.git") };
    assert_eq!(remote("/srv/repo.git", None, local).display(), "/srv/repo.git");
    // A work tree's path, not the `.git` directory inside it.
    let worktree = Transport::Local { git_dir: PathBuf::from("github.com/me/lib").join(".git") };
    assert_eq!(remote("github.com/me/lib", None, worktree).display(), "github.com/me/lib");
}
