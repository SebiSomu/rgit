//! Push over git's smart-HTTP protocol (`git-receive-pack`), checked by real git: after each
//! push the server repository must pass `git fsck` and hold exactly the pushed refs. Covers
//! fast-forwards (sending only the new objects), new branches, annotated tags, deletions,
//! "Everything up-to-date", rejected and forced updates, a server hook that refuses the
//! push, a server that requires a login, and a first push into an empty repository.

use crate::harness::clean_rgit;
use crate::http_server::{clean_git, GitHttpServer, ServerOptions};
use std::path::{Path, PathBuf};

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

/// Commits `content` to `file` in `repo` with git.
fn commit(repo: &Path, file: &str, content: &str, message: &str) {
    std::fs::write(repo.join(file), content).unwrap();
    git(repo, &["add", file]);
    git(repo, &["-c", "user.name=T", "-c", "user.email=t@e", "commit", "-q", "-m", message]);
}

/// Checks the server repository with real git.
fn assert_server_fsck(bare: &Path) {
    let fsck = clean_git().current_dir(bare).args(["fsck", "--strict", "--no-dangling"]).output().unwrap();
    assert!(fsck.status.success() && fsck.stderr.is_empty(), "git fsck on the server: {}", String::from_utf8_lossy(&fsck.stderr));
}

/// How many POSTs to `git-receive-pack` the server has answered so far.
fn receive_pack_posts(server: &GitHttpServer) -> usize {
    server.requests().iter().filter(|r| r.method == "POST" && r.target.ends_with("/git-receive-pack")).count()
}

/// A server with `repo.git` (two commits on `main`), and an rgit clone of it in `copy`.
/// Returns (server, temp dir, bare repository, clone).
fn setup(options: ServerOptions, home: &Path) -> Option<(GitHttpServer, tempfile::TempDir, PathBuf, PathBuf)> {
    let server = GitHttpServer::start_with(options)?;
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let source = base.join("source");
    std::fs::create_dir_all(&source).unwrap();
    git(&source, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(source.join("src")).unwrap();
    commit(&source, "src/lib.txt", &"a line of text\n".repeat(200), "first");
    commit(&source, "README.md", "readme\n", "second");
    let bare = server.root().join("repo.git");
    git(&source, &["clone", "-q", "--bare", &source.to_string_lossy(), &bare.to_string_lossy()]);
    git(&bare, &["config", "http.receivepack", "true"]);
    let (out, err, ok) = rgit(&base, home, &["clone", &server.url("repo.git"), "copy"]);
    assert!(ok, "rgit clone failed:\n{}{}", out, err);
    Some((server, tmp, bare, base.join("copy")))
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
fn push_branches_tags_and_deletions() {
    let home = tempfile::tempdir().unwrap();
    let (server, _tmp, bare, copy) = or_skip!(setup(ServerOptions::default(), home.path()));

    // A fast-forward sends only the new commit, its root tree and the changed file.
    commit(&copy, "README.md", "readme v2\n", "third");
    let (out, err, ok) = rgit(&copy, home.path(), &["push"]);
    assert!(ok, "rgit push failed:\n{}{}", out, err);
    assert!(out.contains("Enumerating objects: 3, done."), "{}", out);
    assert!(out.contains(" main -> main"), "{}", out);
    assert_eq!(git(&bare, &["rev-parse", "main"]), git(&copy, &["rev-parse", "main"]));
    assert_eq!(git(&copy, &["rev-parse", "refs/remotes/origin/main"]), git(&copy, &["rev-parse", "main"]));
    assert_server_fsck(&bare);

    // Nothing new: no push request at all.
    let before = receive_pack_posts(&server);
    let (out, _, ok) = rgit(&copy, home.path(), &["push"]);
    assert!(ok && out.contains("Everything up-to-date"), "{}", out);
    assert_eq!(receive_pack_posts(&server), before);

    // A new branch with a new folder.
    git(&copy, &["checkout", "-q", "-b", "feature"]);
    std::fs::create_dir_all(copy.join("docs/deep")).unwrap();
    commit(&copy, "docs/deep/guide.md", "guide\n", "feature work");
    let (out, err, ok) = rgit(&copy, home.path(), &["push", "-u", "origin", "feature"]);
    assert!(ok, "rgit push feature failed:\n{}{}", out, err);
    assert!(out.contains(" * [new branch]      feature -> feature"), "{}", out);
    assert!(out.contains("branch 'feature' set up to track 'origin/feature'."), "{}", out);
    assert_eq!(git(&bare, &["rev-parse", "feature"]), git(&copy, &["rev-parse", "feature"]));
    assert_eq!(git(&copy, &["rev-parse", "--abbrev-ref", "feature@{upstream}"]), "origin/feature");
    assert_server_fsck(&bare);

    // An annotated tag arrives as a tag (the tag object itself), not as a branch.
    git(&copy, &["-c", "user.name=T", "-c", "user.email=t@e", "tag", "-a", "v1", "-m", "version 1"]);
    let (out, err, ok) = rgit(&copy, home.path(), &["push", "origin", "v1"]);
    assert!(ok, "rgit push v1 failed:\n{}{}", out, err);
    assert!(out.contains(" * [new tag]         v1 -> v1"), "{}", out);
    assert_eq!(git(&bare, &["rev-parse", "refs/tags/v1"]), git(&copy, &["rev-parse", "refs/tags/v1"]));
    assert_eq!(git(&bare, &["cat-file", "-t", "refs/tags/v1"]), "tag");
    assert!(clean_git().current_dir(&bare).args(["rev-parse", "--verify", "-q", "refs/heads/v1"]).output().unwrap().stdout.is_empty());
    assert_server_fsck(&bare);

    // Deleting the branch on the server also drops the remote-tracking ref.
    let (out, err, ok) = rgit(&copy, home.path(), &["push", "origin", ":feature"]);
    assert!(ok, "rgit push :feature failed:\n{}{}", out, err);
    assert!(out.contains(" - [deleted]         feature"), "{}", out);
    assert_eq!(git(&bare, &["for-each-ref", "--format=%(refname)", "refs/heads"]), "refs/heads/main");
    assert!(!copy.join(".git/refs/remotes/origin/feature").exists());

    // Deleting what isn't there fails without asking the server to change anything.
    let before = receive_pack_posts(&server);
    let (_, err, ok) = rgit(&copy, home.path(), &["push", "origin", ":feature"]);
    assert!(!ok && err.contains("remote ref does not exist"), "{}", err);
    assert_eq!(receive_pack_posts(&server), before);
}

#[test]
fn diverged_branches_are_rejected_unless_forced() {
    let home = tempfile::tempdir().unwrap();
    let (server, _tmp, bare, copy) = or_skip!(setup(ServerOptions::default(), home.path()));
    // Someone else pushes to the server.
    let other = copy.parent().unwrap().join("other");
    git(copy.parent().unwrap(), &["clone", "-q", &bare.to_string_lossy(), &other.to_string_lossy()]);
    commit(&other, "README.md", "their change\n", "theirs");
    git(&other, &["push", "-q", "origin", "main"]);
    let theirs = git(&bare, &["rev-parse", "main"]);
    commit(&copy, "README.md", "my change\n", "mine");
    let tracking = git(&copy, &["rev-parse", "refs/remotes/origin/main"]);

    // rgit doesn't have their commit yet: fetch first.
    let before = receive_pack_posts(&server);
    let (_, err, ok) = rgit(&copy, home.path(), &["push"]);
    assert!(!ok, "a push that would lose their commit must fail");
    assert!(err.contains(" ! [rejected]        main -> main (fetch first)"), "{}", err);
    assert_eq!(receive_pack_posts(&server), before, "a rejected update is not sent");
    assert_eq!(git(&bare, &["rev-parse", "main"]), theirs);
    assert_eq!(git(&copy, &["rev-parse", "refs/remotes/origin/main"]), tracking);

    // After a fetch it is a plain non-fast-forward.
    let (_, err, ok) = rgit(&copy, home.path(), &["fetch"]);
    assert!(ok, "{}", err);
    let (_, err, ok) = rgit(&copy, home.path(), &["push"]);
    assert!(!ok && err.contains("(non-fast-forward)"), "{}", err);
    assert_eq!(git(&bare, &["rev-parse", "main"]), theirs);

    // --force replaces their commit.
    let (out, err, ok) = rgit(&copy, home.path(), &["push", "-f"]);
    assert!(ok, "rgit push -f failed:\n{}{}", out, err);
    assert!(out.contains("(forced update)"), "{}", out);
    assert_eq!(git(&bare, &["rev-parse", "main"]), git(&copy, &["rev-parse", "main"]));
    assert_server_fsck(&bare);
}

#[test]
fn a_server_hook_can_refuse_the_push() {
    let home = tempfile::tempdir().unwrap();
    let (_server, _tmp, bare, copy) = or_skip!(setup(ServerOptions::default(), home.path()));
    let hook = bare.join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/bin/sh\necho 'pushes are closed today' >&2\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let before = git(&bare, &["rev-parse", "main"]);
    let tracking = git(&copy, &["rev-parse", "refs/remotes/origin/main"]);
    commit(&copy, "README.md", "refused\n", "refused");

    let (_, err, ok) = rgit(&copy, home.path(), &["push"]);
    assert!(!ok, "the hook refuses the push");
    assert!(err.contains("remote: pushes are closed today"), "the hook's message is shown: {}", err);
    assert!(err.contains(" ! [remote rejected] main -> main (pre-receive hook declined)"), "{}", err);
    assert!(err.contains("error: failed to push some refs to"), "{}", err);
    assert_eq!(git(&bare, &["rev-parse", "main"]), before);
    assert_eq!(git(&copy, &["rev-parse", "refs/remotes/origin/main"]), tracking, "the tracking ref moves only on success");
}

#[test]
fn push_with_a_login_from_the_credential_helper() {
    let home = tempfile::tempdir().unwrap();
    let options = ServerOptions { basic_auth: Some(("sebi".into(), "s3cret".into())), ..Default::default() };
    let server = or_skip!(GitHttpServer::start_with(options));
    let host = server.url("").trim_start_matches("http://").trim_end_matches('/').to_string();
    // rgit's global config is ~/.rgitconfig.
    std::fs::write(home.path().join(".rgitconfig"), "[credential]\n\thelper = store\n").unwrap();
    std::fs::write(home.path().join(".git-credentials"), format!("http://sebi:s3cret@{}\n", host)).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let mine = tmp.path().canonicalize().unwrap().join("mine");
    std::fs::create_dir_all(&mine).unwrap();
    git(&mine, &["init", "-q", "-b", "main"]);
    commit(&mine, "README.md", "hello\n", "first");
    let bare = server.create_bare_repo("login.git");
    git(&mine, &["remote", "add", "origin", &server.url("login.git")]);

    let (out, err, ok) = rgit(&mine, home.path(), &["push", "-u", "origin", "main"]);
    assert!(ok, "rgit push failed:\n{}{}", out, err);
    assert!(out.contains(" * [new branch]      main -> main"), "{}", out);
    assert_eq!(git(&bare, &["rev-parse", "main"]), git(&mine, &["rev-parse", "main"]));
    assert_eq!(git(&mine, &["rev-parse", "--abbrev-ref", "main@{upstream}"]), "origin/main");
    assert_server_fsck(&bare);
    assert!(server.requests().iter().any(|r| r.status == 401), "the server asked for a login");
}
