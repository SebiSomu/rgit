//! Error messages and exit codes.
use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn outside_repository() {
    let mut r = Repo::new();
    for cmd in ["status", "log", "add x", "commit -m x", "branch", "diff", "rev-parse HEAD", "stash", "tag"] {
        r.run(cmd);
    }
    assert_snapshot!(r.finish());
}

#[test]
fn empty_repository() {
    let mut r = Repo::init();
    for cmd in ["log", "show", "diff HEAD", "branch x", "rev-parse HEAD", "reset --hard", "merge main", "rebase main", "stash", "describe", "blame a"] {
        r.run(cmd);
    }
    assert_snapshot!(r.finish());
}

#[test]
fn bad_arguments() {
    let mut r = three_commits();
    for cmd in ["nosuchcommand", "log --no-such-flag", "show nonexistent", "switch nowhere", "checkout nowhere", "merge nowhere", "add missing.txt", "commit", "reset --hard nope", "cat-file -p nope", "tag -d nope", "branch -d nope", "stash pop", "rebase --continue", "cherry-pick --continue", "revert --abort", "bisect good"] {
        r.run(cmd);
    }
    assert_snapshot!(r.finish());
}
