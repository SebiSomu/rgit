//! rgit-only `absorb`: each staged hunk goes into the commit that last changed its lines.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn absorb_places_hunks_by_blame() {
    let mut r = three_commits();
    r.write("b.txt", "bee\nbuzz\n").run("commit -a -m 'fourth commit'");
    r.run("absorb");
    // a.txt line 2 is "second commit"'s, src/lib.txt line 1 "third commit"'s; the appended line
    // and the new file have no owner and stay staged; the unstaged edit to b.txt stays put.
    r.write("a.txt", "one\nTWO\nthree\n").write("src/lib.txt", "lib v2 fixed\n").write("new.txt", "n\n");
    r.run("add a.txt src/lib.txt new.txt").write("b.txt", "bee\nbuzz\nunstaged\n");
    r.run("absorb --dry-run").run("absorb").run("log --oneline");
    r.run("show HEAD~2 --stat").run("show HEAD~1:src/lib.txt").run("status --short").run("diff --cached").run("stash list");
    assert_snapshot!(r.finish());
}

#[test]
fn absorb_conflict_check_undo_and_pushed() {
    let mut r = three_commits();
    // "fourth commit" edits the line right after the one the fix is for: folding it into
    // "second commit" would conflict, so that hunk stays staged.
    r.write("a.txt", "one\ntwo\nthree\n").run("commit -a -m 'fourth commit'");
    r.write("a.txt", "one\nTWO\nthree\n").write("src/lib.txt", "lib v2!\n").run("add a.txt src/lib.txt");
    r.run("absorb").run("status --short").run("log --oneline -2");
    // `rgit undo` takes the whole absorb back.
    r.run("undo").run("status --short").run("log --oneline -2").run("stash list");
    // Commits a remote-tracking branch has are left alone without --force.
    r.run_in("..", "clone repo clone").write("../clone/src/lib.txt", "lib v3\n").run_in("../clone", "add src/lib.txt");
    r.run_in("../clone", "absorb").run_in("../clone", "absorb -f").run_in("../clone", "show HEAD:src/lib.txt");
    assert_snapshot!(r.finish());
}
