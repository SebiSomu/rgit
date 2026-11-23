use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn oops_after_reset_hard() {
    let mut r = three_commits();
    r.write("a.txt", "uncommitted edit\n");
    r.run("reset --hard HEAD~2").run("oops").run("oops 1").run("log --oneline").run("status --short");
    // Right after an undo, the likely mistake is the undo itself.
    r.run("oops");
    assert_snapshot!(r.finish());
}

#[test]
fn oops_deleted_branch() {
    let mut r = three_commits();
    r.run("switch -c topic").write("t.txt", "topic\n").run("add t.txt").run("commit -m topic-work").run("switch main");
    r.run("branch -D topic").write("a.txt", "more\n").run("add a.txt");
    r.run("oops --all").run("oops 2").run("branch");
    assert_snapshot!(r.finish());
}

#[test]
fn oops_merge_in_progress_and_detached_head() {
    let mut r = Repo::init();
    r.write("a.txt", "base\n").run("add a.txt").run("commit -m base");
    r.run("switch -c feat").write("a.txt", "feat\n").run("commit -a -m feat");
    r.run("switch main").write("a.txt", "main\n").run("commit -a -m main");
    r.run("merge feat").run("oops").run("oops 1").run("status --short");
    r.run("switch --detach main").write("d.txt", "d\n").run("add d.txt").run("commit -m experiment").run("oops");
    assert_snapshot!(r.finish());
}

#[test]
fn oops_with_nothing_wrong() {
    let mut r = Repo::init();
    r.run("oops").run("oops 1");
    assert_snapshot!(r.finish());
}
