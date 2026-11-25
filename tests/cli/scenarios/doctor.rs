//! rgit-only `doctor`: repository checks, each with its fix, and `--fix` for the safe ones.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn doctor_healthy_repository() {
    let mut r = three_commits();
    r.run("doctor").run("doctor --quick");
    assert_snapshot!(r.finish());
}

#[test]
fn doctor_finds_problems_and_fixes_the_safe_ones() {
    let mut r = three_commits();
    // No email anywhere: commits would get rgit's placeholder.
    r.set_env("GIT_AUTHOR_EMAIL", "").set_env("GIT_COMMITTER_EMAIL", "");
    r.write(
        ".git/config",
        "[core]\n\trepositoryformatversion = 0\n\thooksPath = missing-hooks\n[remote \"old\"]\n\turl = ../missing\n\tfetch = +refs/heads/*:refs/remotes/old/*\n[branch \"main\"]\n\tremote = ghost\n\tmerge = refs/heads/main\n[broken\norphan = 1\n",
    );
    // A lock a crashed command left an hour ago, and one a running command may hold.
    r.write(".git/index.lock", "").age(".git/index.lock", 3600).write(".git/refs/heads/main.lock", "");
    // A worktree whose directory is gone.
    r.write(".git/worktrees/ghost/gitdir", "../nowhere/ghost/.git\n").write(".git/worktrees/ghost/HEAD", "ref: refs/heads/main\n");
    // A merge left in progress two days ago, and a huge untracked file.
    r.write(".git/MERGE_HEAD", "0000000000000000000000000000000000000000\n").age(".git/MERGE_HEAD", 2 * 86_400);
    r.write("dump.bin", &"x".repeat(2048));
    r.run("doctor --quick --large 1K").run("doctor --quick --large 1K --fix").run("worktree list");
    r.run("doctor --large 1Q");
    assert_snapshot!(r.finish());
}

#[cfg(unix)]
#[test]
fn doctor_hook_not_executable() {
    let mut r = three_commits();
    r.write(".git/hooks/pre-commit", "#!/bin/sh\nexit 0\n").write(".git/hooks/pre-push.sample", "#!/bin/sh\n");
    r.run("doctor --quick").run("doctor --quick --fix").run("doctor --quick");
    assert_snapshot!(r.finish());
}
