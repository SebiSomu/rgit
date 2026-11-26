use super::{diverged, keep_editor_text, three_commits};
use insta::assert_snapshot;

#[test]
fn squash_folds_the_last_commits_into_one() {
    let mut r = three_commits();
    keep_editor_text(&mut r);
    r.write("parser.txt", "numbers\n").run("add parser.txt").run("commit -m 'Add parser' -m 'Handles numbers.'");
    r.write("parser.txt", "numbers\n").write("a.txt", "fixed\n").run("commit -a -m 'fixup! Add parser'");
    r.write("parser.txt", "numbers\nstrings\n").run("commit -a -m 'Parse strings'");
    // A staged change stays staged and out of the squash.
    r.write("b.txt", "staged\n").run("add b.txt");
    // The editor gets git's squash template; the fixup! message is left out.
    r.run("squash 3").run("log --oneline").run("log -1").run("status --short");
    // Undo brings the three commits back.
    r.run("undo").run("log --oneline -4");
    r.run("squash 2 -m 'Parse strings, after the fix'").run("log --oneline -3");
    r.run("squash 2 --no-edit").run("log -1");
    assert_snapshot!(r.finish());
}

#[test]
fn squash_refuses_what_it_should() {
    let mut r = three_commits();
    r.run("squash 1").run("squash 9").run("squash 2 -m ''").run("log --oneline -1");
    // Pushed commits need --force.
    r.run("update-ref refs/remotes/origin/main HEAD~1");
    r.run("squash 2 --no-edit").run("squash 2 --no-edit --force").run("log --oneline");
    assert_snapshot!(r.finish());
}

#[test]
fn squash_everything_into_a_new_root_and_stop_at_merges() {
    let mut r = three_commits();
    r.run("squash 3 --no-edit").run("log --oneline").run("cat-file -p HEAD");
    let mut d = diverged();
    d.run("merge feature -m 'Merge feature'");
    d.write("base.txt", "after\n").run("commit -a -m after");
    d.run("squash 2 --no-edit").run("log --oneline -1");
    assert_snapshot!(r.finish());
    assert_snapshot!(d.finish());
}
