//! Clone and fetch over git's smart-HTTP protocol, checked by real git: the result must pass
//! `git fsck`, have the source's refs, history and files, and pick up new commits on fetch.
//! Runs against protocol v2, v0, and a server that requires a login. Fetches negotiate, so
//! they bring only new objects, also after many local-only commits (several rounds) and
//! into a repository whose history the server doesn't share.

use crate::harness::clean_rgit;
use crate::http_server::{clean_git, GitHttpServer, ServerOptions};
use std::path::Path;

/// Runs the clean `git` in `dir` and returns its stdout, failing the test if it fails.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = clean_git().current_dir(dir).args(args).output().expect("run git");
    assert!(output.status.success(), "git {:?} in {}: {}", args, dir.display(), String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Runs `rgit` in `dir` and returns (stdout, stderr, success).
fn rgit(dir: &Path, home: &Path, args: &[&str]) -> (String, String, bool) {
    let output = clean_rgit(dir, home).args(args).output().expect("run rgit");
    (String::from_utf8_lossy(&output.stdout).into_owned(), String::from_utf8_lossy(&output.stderr).into_owned(), output.status.success())
}

/// A source repository with files in folders, a binary file, several commits (so the pack
/// has deltas) and two branches.
fn source_repo(dir: &Path) {
    std::fs::create_dir_all(dir.join("src/deep")).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    let mut text = String::new();
    for round in 0..5 {
        for line in 0..200 {
            text.push_str(&format!("round {} line {}: the quick brown fox\n", round, line));
        }
        std::fs::write(dir.join("src/deep/big.txt"), &text).unwrap();
        std::fs::write(dir.join("README.md"), format!("readme v{}\n", round)).unwrap();
        std::fs::write(dir.join("logo.bin"), (0..2048u32).map(|i| ((i * 7 + round) % 256) as u8).collect::<Vec<u8>>()).unwrap();
        git(dir, &["add", "-A"]);
        git(dir, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-q", "-m", &format!("round {}", round)]);
    }
    git(dir, &["branch", "side", "HEAD~2"]);
}

/// Serves `source` as `repo.git` on `server`.
fn publish(server: &GitHttpServer, source: &Path) {
    let bare = server.root().join("repo.git");
    git(source, &["clone", "-q", "--bare", &source.to_string_lossy(), &bare.to_string_lossy()]);
    git(&bare, &["config", "http.receivepack", "true"]);
}

/// Checks a clone made by rgit with real git: objects and connectivity, refs, history, files.
fn assert_good_clone(clone: &Path, source: &Path) {
    let fsck = clean_git().current_dir(clone).args(["fsck", "--strict", "--no-dangling"]).output().unwrap();
    assert!(fsck.status.success() && fsck.stderr.is_empty(), "git fsck: {}", String::from_utf8_lossy(&fsck.stderr));
    for (ours, theirs) in [("refs/remotes/origin/main", "main"), ("refs/remotes/origin/side", "side"), ("HEAD", "main")] {
        assert_eq!(git(clone, &["rev-parse", ours]), git(source, &["rev-parse", theirs]), "{} vs {}", ours, theirs);
    }
    assert_eq!(git(clone, &["log", "--format=%H %s", "main"]), git(source, &["log", "--format=%H %s", "main"]));
    assert_eq!(git(clone, &["status", "--porcelain"]), "", "the work tree matches HEAD");
    assert_eq!(std::fs::read(clone.join("logo.bin")).unwrap(), std::fs::read(source.join("logo.bin")).unwrap());
    assert_eq!(git(clone, &["symbolic-ref", "refs/remotes/origin/HEAD"]).trim(), "refs/remotes/origin/main");
    assert_eq!(git(clone, &["rev-parse", "--abbrev-ref", "main@{upstream}"]).trim(), "origin/main");
}

/// Clones from `server` (as `url`), checks the clone, then fetches new commits.
fn clone_then_fetch(server: &GitHttpServer, url: &str, home: &Path, label: &str) {
    let tmp = tempfile::tempdir().unwrap();
    let base = crate::http_server::plain_path(tmp.path());
    let source = base.join("source");
    source_repo(&source);
    publish(server, &source);

    let (out, err, ok) = rgit(&base, home, &["clone", url, "copy"]);
    assert!(ok, "[{}] rgit clone failed:\n{}{}", label, out, err);
    assert!(out.contains("Cloning into 'copy'..."), "[{}] {}", label, out);
    let clone = base.join("copy");
    assert_good_clone(&clone, &source);

    // New work on the server: rgit fetch brings it in.
    std::fs::write(source.join("README.md"), "after the clone\n").unwrap();
    git(&source, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-q", "-am", "after clone"]);
    git(&source, &["push", "-q", &server.root().join("repo.git").to_string_lossy(), "main"]);
    let (out, err, ok) = rgit(&clone, home, &["fetch"]);
    assert!(ok, "[{}] rgit fetch failed:\n{}{}", label, out, err);
    assert!(out.contains("main"), "[{}] fetch output: {}", label, out);
    // Negotiation: only the new commit, its root tree and README's blob come over.
    assert!(out.contains("Receiving objects: 100% (3/3), done."), "[{}] fetch should receive only the new objects: {}", label, out);
    assert_eq!(git(&clone, &["rev-parse", "refs/remotes/origin/main"]), git(&source, &["rev-parse", "main"]));
    let fsck = clean_git().current_dir(&clone).args(["fsck", "--strict", "--no-dangling"]).output().unwrap();
    assert!(fsck.status.success() && fsck.stderr.is_empty(), "[{}] git fsck after fetch: {}", label, String::from_utf8_lossy(&fsck.stderr));

    // Nothing new: everything is up to date and no pack is requested.
    let before = server.requests().len();
    let (out, _, ok) = rgit(&clone, home, &["fetch"]);
    assert!(ok && out.contains("[up to date]"), "[{}] {}", label, out);
    // Only ref discovery: GET info/refs, plus v2's ls-refs POST. A pack request would be one more POST.
    let posts = server.requests()[before..].iter().filter(|r| r.method == "POST" && r.status == 200).count();
    let discovery_posts = if label == "v0" { 0 } else { 1 };
    assert_eq!(posts, discovery_posts, "[{}] an up-to-date fetch asks for no pack", label);
}

macro_rules! or_skip {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => {
                eprintln!("skipping: git http-backend is not available");
                return;
            }
        }
    };
}

#[test]
fn clone_and_fetch_over_protocol_v2() {
    let server = or_skip!(GitHttpServer::start());
    let home = tempfile::tempdir().unwrap();
    clone_then_fetch(&server, &server.url("repo.git"), home.path(), "v2");
    assert!(server.requests().iter().any(|r| r.headers.get("git-protocol").map(String::as_str) == Some("version=2") && r.target.ends_with("/git-upload-pack")));
}

#[test]
fn clone_and_fetch_over_protocol_v0() {
    let server = or_skip!(GitHttpServer::start_with(ServerOptions { force_v0: true, ..Default::default() }));
    let home = tempfile::tempdir().unwrap();
    clone_then_fetch(&server, &server.url("repo.git"), home.path(), "v0");
}

#[test]
fn clone_and_fetch_with_a_login_from_the_credential_helper() {
    let server = or_skip!(GitHttpServer::start_with(ServerOptions { basic_auth: Some(("sebi".into(), "s3cret".into())), ..Default::default() }));
    let home = tempfile::tempdir().unwrap();
    let host = server.url("").trim_start_matches("http://").trim_end_matches('/').to_string();
    // rgit's global config is ~/.rgitconfig.
    std::fs::write(home.path().join(".rgitconfig"), "[credential]\n\thelper = store\n").unwrap();
    std::fs::write(home.path().join(".git-credentials"), format!("http://sebi:s3cret@{}\n", host)).unwrap();
    clone_then_fetch(&server, &server.url("repo.git"), home.path(), "login");
    // The stored login was used, and kept.
    assert!(std::fs::read_to_string(home.path().join(".git-credentials")).unwrap().contains("s3cret"));
}

#[test]
fn cloning_an_empty_repository() {
    let server = or_skip!(GitHttpServer::start());
    server.create_bare_repo("empty.git");
    let home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let (out, err, ok) = rgit(tmp.path(), home.path(), &["clone", &server.url("empty.git")]);
    assert!(ok, "{}{}", out, err);
    assert!(err.contains("You appear to have cloned an empty repository."), "{}", err);
    let clone = tmp.path().join("empty");
    // HEAD names the server's default branch, though it has no commits yet.
    assert_eq!(git(&clone, &["symbolic-ref", "HEAD"]).trim(), "refs/heads/main");
}

/// `count` local-only commits on a branch `local-work` of `repo` (on top of `from`, or a new
/// root), newer than anything on the server, made with git fast-import so it stays quick.
fn local_only_commits(repo: &Path, count: usize, from: Option<&str>) {
    let mut stream = String::new();
    for i in 0..count {
        let content = format!("local {}\n", i);
        stream.push_str("commit refs/heads/local-work\n");
        stream.push_str(&format!("committer T <t@e> {} +0000\n", 2_000_000_000 + i));
        stream.push_str("data 6\nlocal\n");
        if let (0, Some(from)) = (i, from) {
            stream.push_str(&format!("from {}\n", from));
        }
        stream.push_str(&format!("M 100644 inline local.txt\ndata {}\n{}\n", content.len(), content));
    }
    let mut child = clean_git()
        .current_dir(repo)
        .args(["fast-import", "--quiet"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("run git fast-import");
    use std::io::Write;
    child.stdin.take().unwrap().write_all(stream.as_bytes()).unwrap();
    assert!(child.wait().unwrap().success(), "git fast-import");
}

/// A clone with many commits the server doesn't have still fetches only the new objects:
/// the negotiation takes several rounds to get past the local-only commits.
fn fetch_after_local_work(options: ServerOptions, label: &str) {
    let v0 = options.force_v0;
    let server = or_skip!(GitHttpServer::start_with(options));
    let home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let base = crate::http_server::plain_path(tmp.path());
    let source = base.join("source");
    source_repo(&source);
    publish(&server, &source);
    let (out, err, ok) = rgit(&base, home.path(), &["clone", &server.url("repo.git"), "copy"]);
    assert!(ok, "[{}] rgit clone failed:\n{}{}", label, out, err);
    let clone = base.join("copy");
    local_only_commits(&clone, 300, Some("refs/heads/main"));

    std::fs::write(source.join("README.md"), "after local work\n").unwrap();
    git(&source, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-q", "-am", "server work"]);
    git(&source, &["push", "-q", &server.root().join("repo.git").to_string_lossy(), "main"]);

    let before = server.requests().len();
    let (out, err, ok) = rgit(&clone, home.path(), &["fetch"]);
    assert!(ok, "[{}] rgit fetch failed:\n{}{}", label, out, err);
    assert!(out.contains("Receiving objects: 100% (3/3), done."), "[{}] {}", label, out);
    assert_eq!(git(&clone, &["rev-parse", "refs/remotes/origin/main"]), git(&source, &["rev-parse", "main"]));
    let fsck = clean_git().current_dir(&clone).args(["fsck", "--strict", "--no-dangling"]).output().unwrap();
    assert!(fsck.status.success() && fsck.stderr.is_empty(), "[{}] git fsck: {}", label, String::from_utf8_lossy(&fsck.stderr));
    // Rounds of 16, 32, 64, 128, 256 haves get past the 300 local commits.
    let packs = server.requests()[before..].iter().filter(|r| r.method == "POST" && r.status == 200).count() - if v0 { 0 } else { 1 };
    assert!(packs >= 3, "[{}] expected several negotiation rounds, got {}", label, packs);
}

#[test]
fn fetch_negotiates_past_local_only_commits_over_v2() {
    fetch_after_local_work(ServerOptions::default(), "v2");
}

#[test]
fn fetch_negotiates_past_local_only_commits_over_v0() {
    fetch_after_local_work(ServerOptions { force_v0: true, ..Default::default() }, "v0");
}

/// A repository with its own unrelated history fetches the server's whole history: every
/// `have` is answered with NAK and the negotiation ends when there is nothing left to offer.
fn fetch_into_unrelated_history(options: ServerOptions, label: &str) {
    let server = or_skip!(GitHttpServer::start_with(options));
    let home = tempfile::tempdir().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let base = crate::http_server::plain_path(tmp.path());
    let source = base.join("source");
    source_repo(&source);
    publish(&server, &source);
    let mine = base.join("mine");
    std::fs::create_dir_all(&mine).unwrap();
    git(&mine, &["init", "-q", "-b", "main"]);
    local_only_commits(&mine, 40, None);
    git(&mine, &["symbolic-ref", "HEAD", "refs/heads/local-work"]);
    git(&mine, &["remote", "add", "origin", &server.url("repo.git")]);

    let (out, err, ok) = rgit(&mine, home.path(), &["fetch"]);
    assert!(ok, "[{}] rgit fetch failed:\n{}{}", label, out, err);
    assert!(out.contains(" * [new branch]      main"), "[{}] {}", label, out);
    assert_eq!(git(&mine, &["rev-parse", "refs/remotes/origin/main"]), git(&source, &["rev-parse", "main"]));
    assert_eq!(git(&mine, &["rev-list", "--count", "refs/remotes/origin/main"]).trim(), "5");
    let fsck = clean_git().current_dir(&mine).args(["fsck", "--strict", "--no-dangling"]).output().unwrap();
    assert!(fsck.status.success() && fsck.stderr.is_empty(), "[{}] git fsck: {}", label, String::from_utf8_lossy(&fsck.stderr));
}

#[test]
fn fetch_into_unrelated_history_over_v2() {
    fetch_into_unrelated_history(ServerOptions::default(), "v2");
}

#[test]
fn fetch_into_unrelated_history_over_v0() {
    fetch_into_unrelated_history(ServerOptions { force_v0: true, ..Default::default() }, "v0");
}
