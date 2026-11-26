use super::{diverged, three_commits};
use insta::assert_snapshot;

#[test]
fn drop_removes_a_commit_and_replays_the_rest() {
    let mut r = three_commits();
    r.write("debug.txt", "temporary\n").run("add debug.txt").run("commit -m 'Add debug output'");
    r.write("a.txt", "one\ntwo\nthree\n").run("commit -a -m 'fourth commit'");
    // Uncommitted work (staged and not) comes through untouched.
    r.write("b.txt", "staged\n").run("add b.txt").write("src/lib.txt", "unstaged\n");
    r.run("drop HEAD~1").run("log --oneline").run("ls-files").run("status --short");
    // The newest commit: nothing to replay.
    r.run("drop HEAD").run("log --oneline -2").run("status --short");
    r.run("undo").run("log --oneline -2");
    assert_snapshot!(r.finish());
}

#[test]
fn drop_stops_on_a_conflict_like_a_rebase() {
    let mut r = three_commits();
    r.write("a.txt", "one\nTWO\n").run("commit -a -m 'Shout two'");
    r.write("a.txt", "one\nTWO!\n").run("commit -a -m 'More shouting'");
    // The later commit changed the line the dropped one wrote.
    r.run("drop HEAD~1").run("status --short");
    r.run("rebase --abort").run("log --oneline -3");
    // Again, resolving it this time.
    r.run("drop HEAD~1").write("a.txt", "one\ntwo!\n").run("add a.txt").run("rebase --continue").run("log --oneline -3").show_file("a.txt");
    assert_snapshot!(r.finish());
}

#[test]
fn drop_refuses_what_it_should() {
    let mut r = three_commits();
    r.run("drop HEAD~2").run("drop nope");
    r.run("switch -c side").write("side.txt", "s\n").run("add side.txt").run("commit -m side").run("switch main");
    r.run("drop side");
    r.run("update-ref refs/remotes/origin/main HEAD~1");
    r.run("drop HEAD~1").run("drop HEAD~1 --force").run("log --oneline");
    let mut d = diverged();
    d.run("merge feature -m 'Merge feature'");
    d.write("base.txt", "after\n").run("commit -a -m after");
    d.run("drop HEAD~1").run("drop HEAD~2");
    assert_snapshot!(r.finish());
    assert_snapshot!(d.finish());
}
