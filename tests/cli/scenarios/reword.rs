use super::{diverged, keep_editor_text, three_commits};
use insta::assert_snapshot;

#[test]
fn reword_head_and_older_commits() {
    let mut r = three_commits();
    // An editor that saves the message unchanged: nothing to do.
    keep_editor_text(&mut r);
    r.run("reword");
    r.write("b.txt", "staged\n").run("add b.txt");
    r.run("rev-parse HEAD^{tree}");
    r.run("reword -m 'Third commit, better said'").run("log --oneline");
    // An older one: the commits after it are rewritten with the same trees.
    r.run("reword HEAD~2 -m 'First commit' -m 'Now with a body.'").run("log --oneline").run("log -1 --format=%B HEAD~2");
    r.run("rev-parse HEAD^{tree}").run("status --short");
    r.run("undo").run("log --oneline");
    assert_snapshot!(r.finish());
}

#[test]
fn reword_refuses_what_it_should() {
    let mut r = three_commits();
    r.run("reword HEAD -m ''").run("reword nope -m x");
    r.run("switch -c side").write("side.txt", "s\n").run("add side.txt").run("commit -m side").run("switch main");
    r.run("reword side -m 'not here'");
    r.run("update-ref refs/remotes/origin/main HEAD~1");
    r.run("reword HEAD~1 -m 'pushed already'").run("reword HEAD -m 'not pushed yet'");
    r.run("reword HEAD~1 -m 'pushed, but forced' --force").run("log --oneline");
    assert_snapshot!(r.finish());
}

#[test]
fn reword_below_a_merge_keeps_its_other_parent() {
    let mut d = diverged();
    d.run("merge feature -m 'Merge feature'");
    d.run("log --oneline --graph");
    d.run("reword HEAD^1 -m 'main work, reworded'").run("log --oneline --graph");
    assert_snapshot!(d.finish());
}
