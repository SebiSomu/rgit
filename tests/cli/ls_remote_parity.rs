//! `rgit ls-remote` against real `git ls-remote`: the same repository through every
//! transport (bare and non-bare paths, a bundle, smart HTTP v2 and v0, an empty repository),
//! with the same flags, must give byte-identical output and the same exit code.

use crate::harness::clean_rgit as rgit;
use crate::http_server::{clean_git, GitHttpServer, ServerOptions};
use std::path::Path;
use std::process::Command;

/// Runs the clean `git` in `dir`, failing the test if it fails.
fn git(dir: &Path, args: &[&str]) {
    let output = clean_git().current_dir(dir).args(args).output().expect("run git");
    assert!(output.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&output.stderr));
}

/// A repository with the kinds of refs ls-remote has to get right: branches, annotated,
/// lightweight and nested tags, packed refs with a loose one overriding them, notes, and a
/// symbolic remote-tracking HEAD.
fn rich_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    let id = ["-c", "user.name=T", "-c", "user.email=t@e"];
    let commit = |msg: &str| {
        let mut args = id.to_vec();
        args.extend(["commit", "-q", "--allow-empty", "-m", msg]);
        git(dir, &args);
    };
    git(dir, &["init", "-q", "-b", "main"]);
    commit("one");
    git(dir, &["branch", "feature"]);
    git(dir, &["branch", "feat/nested"]);
    let mut tag = id.to_vec();
    tag.extend(["tag", "-a", "v1", "-m", "annotated"]);
    git(dir, &tag);
    git(dir, &["tag", "light"]);
    let mut nested = id.to_vec();
    nested.extend(["tag", "-a", "v1-outer", "v1", "-m", "tag of a tag"]);
    git(dir, &nested);
    git(dir, &["pack-refs", "--all"]);
    commit("two"); // main moves on: its loose ref now overrides the packed one
    let mut tag2 = id.to_vec();
    tag2.extend(["tag", "-a", "v2", "-m", "loose annotated"]);
    git(dir, &tag2);
    let mut note = id.to_vec();
    note.extend(["notes", "add", "-m", "a note"]);
    git(dir, &note);
    git(dir, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(dir, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
}

/// (stdout, exit code) of a command.
fn run(mut command: Command) -> (String, i32) {
    let output = command.output().expect("run command");
    (String::from_utf8_lossy(&output.stdout).into_owned(), output.status.code().unwrap_or(-1))
}

const FLAG_SETS: &[&[&str]] = &[
    &[],
    &["--symref"],
    &["--heads"],
    &["--tags"],
    &["--refs"],
    &["--heads", "--tags"],
    &["--tags", "--refs"],
    &["--symref", "--refs"],
    &["main"],
    &["v1"],
    &["HEAD"],
    &["feat*"],
    &["--exit-code", "no-such-ref"],
];

#[test]
fn ls_remote_matches_git_on_every_transport() {
    let Some(server) = GitHttpServer::start() else {
        eprintln!("skipping: git http-backend is not available");
        return;
    };
    let Some(v0_server) = GitHttpServer::start_with(ServerOptions { force_v0: true, ..Default::default() }) else { return };
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let (work, home, cwd) = (base.join("work"), base.join("home"), base.join("cwd"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    rich_repo(&work);
    for root in [server.root(), v0_server.root()] {
        git(&base, &["clone", "-q", "--bare", &work.to_string_lossy(), &root.join("repo.git").to_string_lossy()]);
        // A bare clone has no remote-tracking refs; copy that symbolic ref across too.
        git(&root.join("repo.git"), &["update-ref", "refs/remotes/origin/main", "main"]);
        git(&root.join("repo.git"), &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main"]);
    }
    server.create_bare_repo("empty.git");
    git(&work, &["bundle", "create", "-q", &base.join("all.bundle").to_string_lossy(), "--all"]);

    let targets = [
        ("bare repository", server.root().join("repo.git").to_string_lossy().into_owned()),
        ("work tree", work.to_string_lossy().into_owned()),
        ("bundle", base.join("all.bundle").to_string_lossy().into_owned()),
        ("smart HTTP v2", server.url("repo.git")),
        ("smart HTTP v0", v0_server.url("repo.git")),
        ("empty repository over HTTP", server.url("empty.git")),
    ];
    let mut checked = 0;
    for (label, target) in &targets {
        for flags in FLAG_SETS {
            let mut args: Vec<&str> = vec!["ls-remote"];
            // Options first, then the repository, then patterns, as in git's usage.
            let (options, patterns): (Vec<&str>, Vec<&str>) = flags.iter().partition(|f| f.starts_with('-'));
            args.extend(options);
            args.push(target);
            args.extend(patterns);

            let mut expected = clean_git();
            expected.current_dir(&cwd).args(&args);
            let mut actual = rgit(&cwd, &home);
            actual.args(&args);
            let (want, got) = (run(expected), run(actual));
            assert_eq!(got, want, "`{}` over {}\n--- git ---\n{}--- rgit ---\n{}", args.join(" "), label, want.0, got.0);
            checked += 1;
        }
    }
    assert_eq!(checked, targets.len() * FLAG_SETS.len());
}

