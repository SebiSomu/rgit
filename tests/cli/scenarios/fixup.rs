//! rgit-only `fixup`: fold staged changes into an earlier commit in one step.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn fixup_pick_and_rewrite() {
    let mut r = three_commits();
    r.run("fixup HEAD~1");
    // A typo in "second commit"'s a.txt, plus an unrelated unstaged edit that must survive.
    r.write("a.txt", "one\nTWO\n").run("add a.txt").write("b.txt", "bee (unstaged)\n");
    r.run_stdin("fixup", "\n");
    r.run_stdin("fixup", "2\n").run("log --oneline").run("show --stat HEAD~1").run("status --short").run("stash list");
    // HEAD itself is just amended; -a stages tracked changes first.
    r.run("fixup -a HEAD").run("log --oneline -1").run("status --short");
    r.write("a.txt", "x\n").run("add a.txt").run("fixup nope").run_stdin("fixup", "9\n");
    assert_snapshot!(r.finish());
}

#[test]
fn fixup_conflict_undo_and_pushed() {
    let mut r = three_commits();
    r.write("a.txt", "one\n2\n").run("commit -a -m 'fourth commit'");
    // A fix meant for "second commit" that touches the line "fourth commit" changed: it stops
    // on the conflict, says how to go on, and `rgit undo` puts everything back.
    r.write("a.txt", "one\n2 fixed\n").run("add a.txt").write("b.txt", "unstaged\n");
    r.run("fixup HEAD~2").run("status --short").run("undo").run("status --short").run("log --oneline").run("stash list");
    // Commits a remote-tracking branch has are refused without --force.
    r.run_in("..", "clone repo clone").write("../clone/b.txt", "fix\n").run_in("../clone", "add b.txt");
    r.run_in("../clone", "fixup HEAD~1").run_in("../clone", "fixup -f HEAD~1").run_in("../clone", "log --oneline");
    assert_snapshot!(r.finish());
}
