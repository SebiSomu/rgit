//! Self-tests for the test git server in `http_server.rs`: real `git` as the client proves
//! the server speaks smart HTTP correctly, before any rgit code relies on it.

use crate::http_server::{clean_git, GitHttpServer, ServerOptions};
use std::path::Path;

/// A work tree with one commit, made with the clean `git`.
fn work_repo(dir: &Path) {
    let run = |args: &[&str]| {
        let status = clean_git().current_dir(dir).args(args).status().expect("run git");
        assert!(status.success(), "git {:?} failed", args);
    };
    std::fs::create_dir_all(dir).unwrap();
    run(&["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
    run(&["add", "a.txt"]);
    run(&["-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "-q", "-m", "first"]);
}

/// Runs the clean `git` and returns (success, stderr).
fn git_in(dir: &Path, args: &[&str]) -> (bool, String) {
    let output = clean_git().current_dir(dir).args(args).output().expect("run git");
    (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned())
}

#[test]
fn real_git_pushes_and_clones_over_http() {
    let Some(server) = GitHttpServer::start() else {
        eprintln!("skipping: git http-backend is not available");
        return;
    };
    server.create_bare_repo("repo.git");
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    work_repo(&work);

    let (ok, err) = git_in(&work, &["push", &server.url("repo.git"), "main"]);
    assert!(ok, "push over HTTP failed: {}", err);

    let (ok, err) = git_in(tmp.path(), &["clone", "-q", &server.url("repo.git"), "copy"]);
    assert!(ok, "clone over HTTP failed: {}", err);
    assert_eq!(std::fs::read_to_string(tmp.path().join("copy/a.txt")).unwrap(), "hello\n");

    let requests = server.requests();
    let targets: Vec<&str> = requests.iter().map(|r| r.target.as_str()).collect();
    assert!(targets.contains(&"/repo.git/info/refs?service=git-receive-pack"), "{:?}", targets);
    assert!(targets.contains(&"/repo.git/git-receive-pack"), "{:?}", targets);
    assert!(targets.contains(&"/repo.git/git-upload-pack"), "{:?}", targets);
    assert!(requests.iter().all(|r| r.status == 200), "{:?}", requests);
    // git asks for protocol v2 when fetching; the server passes that on to http-backend.
    let discovery = requests.iter().find(|r| r.target == "/repo.git/info/refs?service=git-upload-pack").unwrap();
    assert_eq!(discovery.headers.get("git-protocol").map(String::as_str), Some("version=2"));
}

#[test]
fn basic_auth_is_required_when_configured() {
    let options = ServerOptions { basic_auth: Some(("sebi".into(), "s3cret".into())), ..Default::default() };
    let Some(server) = GitHttpServer::start_with(options) else {
        eprintln!("skipping: git http-backend is not available");
        return;
    };
    let bare = server.create_bare_repo("private.git");
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    work_repo(&work);
    let (ok, err) = git_in(&work, &["push", "-q", &bare.to_string_lossy(), "main"]);
    assert!(ok, "seeding the bare repository failed: {}", err);

    let (ok, err) = git_in(tmp.path(), &["clone", "-q", &server.url("private.git"), "denied"]);
    assert!(!ok, "clone without credentials should fail");
    assert!(err.contains("could not read Username") || err.contains("Authentication failed"), "{}", err);
    assert_eq!(server.requests().last().map(|r| r.status), Some(401));

    let (ok, err) = git_in(tmp.path(), &["clone", "-q", &server.url_with_login("private.git", "sebi", "s3cret"), "allowed"]);
    assert!(ok, "clone with credentials failed: {}", err);
    assert!(server.requests().iter().any(|r| r.status == 200 && r.headers.contains_key("authorization")));

    let (ok, _) = git_in(tmp.path(), &["clone", "-q", &server.url_with_login("private.git", "sebi", "wrong"), "wrong"]);
    assert!(!ok, "a wrong password must be refused");
}

#[test]
fn missing_repositories_are_404() {
    let Some(server) = GitHttpServer::start() else {
        eprintln!("skipping: git http-backend is not available");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let (ok, err) = git_in(tmp.path(), &["ls-remote", &server.url("nope.git")]);
    assert!(!ok);
    assert!(err.contains("not found") || err.contains("404"), "{}", err);
    assert_eq!(server.requests().last().map(|r| r.status), Some(404));
}
