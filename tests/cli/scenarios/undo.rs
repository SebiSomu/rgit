use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn undo_redo_commit() {
    let mut r = three_commits();
    r.write("c.txt", "c\n").run("add c.txt").run("commit -m fourth");
    r.run("undo --list").run("undo -n").run("undo").run("log --oneline").run("status --short");
    r.checkpoint("after undo");
    r.run("redo").run("log --oneline");
    // A new command after an undo forgets what could be redone.
    r.run("undo").run("branch side").run("redo").run("undo --list");
    assert_snapshot!(r.finish());
}

#[test]
fn undo_reset_hard_restores_uncommitted_work() {
    let mut r = three_commits();
    r.write("a.txt", "uncommitted edit\n").write("notes.txt", "untracked\n");
    r.run("reset --hard HEAD~2").run("undo").run("log --oneline").run("status --short");
    assert_snapshot!(r.finish());
}

#[test]
fn undo_branch_delete_and_conflicted_merge() {
    let mut r = Repo::init();
    r.write("a.txt", "base\n").run("add a.txt").run("commit -m base");
    r.run("switch -c feat").write("a.txt", "feat\n").run("commit -a -m feat");
    r.run("switch main").write("a.txt", "main\n").run("commit -a -m main");
    r.run("branch -D feat").run("undo").run("branch");
    r.run("merge feat").checkpoint("conflicted merge");
    r.run("undo").run("status");
    assert_snapshot!(r.finish());
}

#[test]
fn undo_refuses_when_changed_since() {
    let mut r = three_commits();
    // a.txt is rewritten by the reset, then edited again: undo must not silently lose that edit.
    r.run("reset --hard HEAD~2").write("a.txt", "edited after the reset\n");
    r.run("undo").run("log --oneline -1").run("undo --force").run("log --oneline -1").run("status --short");
    assert_snapshot!(r.finish());
}

#[test]
fn undo_switch_keeps_unrelated_edits() {
    let mut r = three_commits();
    r.run("switch -c topic").run("switch main");
    r.write("a.txt", "edited after the switch\n");
    r.run("undo").run("undo").run("branch").run("status --short");
    assert_snapshot!(r.finish());
}

#[test]
fn undo_with_empty_log() {
    let mut r = Repo::init();
    r.run("undo").run("redo").run("undo --list");
    assert_snapshot!(r.finish());
}
