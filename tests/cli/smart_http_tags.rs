//! Tags over smart HTTP, checked by real git. By default a fetch (and so a clone) brings the
//! tags that point at something the repository has afterwards, as git's auto-follow does;
//! `--tags` brings all of them, `--no-tags` none, and an existing local tag is never moved.

use crate::harness::clean_rgit;
use crate::http_server::{clean_git, GitHttpServer, ServerOptions};
use std::path::Path;

/// Runs the clean `git` in `dir` and returns its stdout, failing the test if it fails.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = clean_git().current_dir(dir).args(args).output().expect("run git");
    assert!(output.status.success(), "git {:?} in {}: {}", args, dir.display(), String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Runs `rgit` in `dir` and returns (stdout, stderr, success).
fn rgit(dir: &Path, home: &Path, args: &[&str]) -> (String, String, bool) {
    let output = clean_rgit(dir, home).args(args).output().expect("run rgit");
    (String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned(), output.status.success())
}

/// `git` with an identity, for commits and annotated tags.
fn git_as_t(dir: &Path, args: &[&str]) -> String {
    let mut all = vec!["-c", "user.name=T", "-c", "user.email=t@e"];
    all.extend_from_slice(args);
    git(dir, &all)
}

fn commit(dir: &Path, content: &str) {
    std::fs::write(dir.join("file.txt"), content).unwrap();
    git(dir, &["add", "file.txt"]);
    git_as_t(dir, &["commit", "-q", "-m", content]);
}

/// The tags of `repo` as `name value` lines.
fn tags(repo: &Path) -> Vec<String> {
    let out = git(repo, &["for-each-ref", "--format=%(refname:short) %(objectname)", "refs/tags"]);
    out.lines().map(str::to_string).collect()
}

fn has_tag(repo: &Path, name: &str) -> bool {
    tags(repo).iter().any(|t| t.split(' ').next() == Some(name))
}

fn assert_fsck(repo: &Path) {
    let fsck = clean_git().current_dir(repo).args(["fsck", "--strict", "--no-dangling"]).output().unwrap();
    assert!(fsck.status.success() && fsck.stderr.is_empty(), "git fsck: {}", String::from_utf8_lossy(&fsck.stderr));
}

fn tags_follow_the_history(options: ServerOptions, label: &str) {
    let Some(server) = GitHttpServer::start_with(options) else {
        eprintln!("skipping: git http-backend is not available");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let base = crate::http_server::plain_path(tmp.path());
    let source = base.join("source");
    std::fs::create_dir_all(&source).unwrap();
    git(&source, &["init", "-q", "-b", "main"]);
    commit(&source, "one");
    git(&source, &["tag", "light"]);
    commit(&source, "two");
    git_as_t(&source, &["tag", "-a", "v1", "-m", "version 1"]);
    // A tag on a commit no branch reaches: only `--tags` brings it.
    git(&source, &["checkout", "-q", "--detach"]);
    commit(&source, "orphan");
    git_as_t(&source, &["tag", "-a", "orphan", "-m", "off every branch"]);
    git(&source, &["checkout", "-q", "main"]);
    let bare = server.root().join("repo.git");
    git(&source, &["clone", "-q", "--bare", &source.to_string_lossy(), &bare.to_string_lossy()]);
    let publish = |args: &[&str]| {
        let mut all = vec!["push", "-q", "--force", bare.to_str().unwrap()];
        all.extend_from_slice(args);
        git(&source, &all);
    };

    // Clone follows the tags of the history it brings, annotated ones as tag objects.
    let (out, err, ok) = rgit(&base, home.path(), &["clone", &server.url("repo.git"), "copy"]);
    assert!(ok, "[{}] rgit clone failed:\n{}{}", label, out, err);
    let copy = base.join("copy");
    assert!(out.contains(" * [new tag]         v1"), "[{}] {}", label, out);
    assert!(has_tag(&copy, "light") && has_tag(&copy, "v1") && !has_tag(&copy, "orphan"), "[{}] {:?}", label, tags(&copy));
    assert_eq!(git(&copy, &["cat-file", "-t", "v1"]), "tag");
    assert_eq!(git(&copy, &["rev-parse", "v1"]), git(&source, &["rev-parse", "v1"]));
    assert_fsck(&copy);

    // A new tag on a commit the clone already has.
    git_as_t(&source, &["tag", "-a", "v1.1", "light", "-m", "late tag"]);
    publish(&["refs/tags/v1.1"]);
    let (out, err, ok) = rgit(&copy, home.path(), &["fetch"]);
    assert!(ok, "[{}] {}{}", label, out, err);
    assert!(out.contains(" * [new tag]         v1.1"), "[{}] {}", label, out);
    assert!(out.contains("Receiving objects: 100% (1/1), done."), "[{}] only the tag object: {}", label, out);

    // A tag that comes with new commits arrives in the same pack.
    commit(&source, "three");
    git_as_t(&source, &["tag", "-a", "v2", "-m", "version 2"]);
    publish(&["main", "refs/tags/v2"]);
    // `--no-tags` leaves it out...
    let (out, err, ok) = rgit(&copy, home.path(), &["fetch", "--no-tags"]);
    assert!(ok, "[{}] {}{}", label, out, err);
    assert!(!has_tag(&copy, "v2") && !out.contains("[new tag]"), "[{}] {}", label, out);
    // ...and a plain fetch then brings just the tag.
    let (out, err, ok) = rgit(&copy, home.path(), &["fetch"]);
    assert!(ok, "[{}] {}{}", label, out, err);
    assert!(out.contains(" * [new tag]         v2"), "[{}] {}", label, out);
    assert_eq!(git(&copy, &["rev-parse", "v2"]), git(&source, &["rev-parse", "v2"]));
    assert_fsck(&copy);

    // A plain fetch of new commits and their tag: the tag object comes in the same pack
    // (include-tag), so there is a single pack request.
    commit(&source, "four");
    git_as_t(&source, &["tag", "-a", "v3", "-m", "version 3"]);
    publish(&["main", "refs/tags/v3"]);
    let pack_requests = || server.requests().iter().filter(|r| r.method == "POST" && r.target.ends_with("/git-upload-pack")).count();
    let before = pack_requests();
    let (out, err, ok) = rgit(&copy, home.path(), &["fetch"]);
    assert!(ok, "[{}] {}{}", label, out, err);
    assert!(out.contains(" * [new tag]         v3"), "[{}] {}", label, out);
    assert!(out.contains("Receiving objects: 100% (4/4), done."), "[{}] commit, tree, blob, tag: {}", label, out);
    // v2: ls-refs, then one fetch the server answers "ready" to. v0 over HTTP: a round of
    // haves, then the `done` round. A separate download for the tag would add more.
    assert_eq!(pack_requests() - before, 2, "[{}] one pack for commits and tag", label);
    assert_fsck(&copy);

    // `--tags` brings the tag no branch reaches, with its commit.
    let (out, err, ok) = rgit(&copy, home.path(), &["fetch", "--tags"]);
    assert!(ok, "[{}] {}{}", label, out, err);
    assert!(out.contains(" * [new tag]         orphan"), "[{}] {}", label, out);
    assert_eq!(git(&copy, &["rev-parse", "orphan^{commit}"]), git(&source, &["rev-parse", "orphan^{commit}"]));
    assert_fsck(&copy);

    // A tag moved on the server is never moved here: silently by default, reported by --tags.
    let before = git(&copy, &["rev-parse", "v1"]);
    git_as_t(&source, &["tag", "-f", "-a", "v1", "HEAD", "-m", "moved"]);
    publish(&["refs/tags/v1"]);
    let (out, err, ok) = rgit(&copy, home.path(), &["fetch"]);
    assert!(ok && !out.contains("v1"), "[{}] {}{}", label, out, err);
    let (out, err, ok) = rgit(&copy, home.path(), &["fetch", "--tags"]);
    assert!(!ok, "[{}] a clobbered tag fails the fetch", label);
    assert!(out.contains(" ! [rejected]        v1") && out.contains("(would clobber existing tag)"), "[{}] {}", label, out);
    assert!(err.contains("some local refs could not be updated"), "[{}] {}", label, err);
    assert_eq!(git(&copy, &["rev-parse", "v1"]), before);
}

#[test]
fn tags_follow_the_history_over_v2() {
    tags_follow_the_history(ServerOptions::default(), "v2");
}

#[test]
fn tags_follow_the_history_over_v0() {
    tags_follow_the_history(ServerOptions { force_v0: true, ..Default::default() }, "v0");
}
