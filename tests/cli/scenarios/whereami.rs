//! rgit-only `whereami`: one screen on where the repository stands and what to run next.
use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn whereami_everyday_states() {
    let mut r = Repo::init();
    r.run("whereami");
    r.write("a.txt", "one\ntwo\n").write("src/lib.txt", "lib v2\n").write("b.txt", "bee\n").run("add .").run("commit -m 'first commit'").run("whereami");
    r.write("a.txt", "one\nTWO\n").write("src/lib.txt", "lib v3\n").write("new.txt", "n\n").run("add a.txt").run("whereami");
    r.run("commit -a -m 'fourth commit'").run("whereami");
    r.write("b.txt", "bee?\n").run("stash push -m parked").run("wip 'half done'").run("whereami");
    // A clone tracks origin/main: ahead, then diverged after a fetch.
    r.run_in("..", "clone repo clone").run_in("../clone", "whereami");
    r.write("../clone/c.txt", "c\n").run_in("../clone", "add c.txt").run_in("../clone", "commit -m 'clone commit'").run_in("../clone", "whereami");
    r.write("b.txt", "bee 2\n").run("commit -a -m 'fifth commit'").run_in("../clone", "fetch").run_in("../clone", "whereami");
    r.run_in("../clone", "switch -c topic").run_in("../clone", "whereami");
    r.run("switch --detach HEAD~1").run("whereami");
    assert_snapshot!(r.finish());
}

#[test]
fn whereami_operations_in_progress() {
    let mut r = three_commits();
    r.run("switch -c side").write("a.txt", "one\nside\n").run("commit -a -m 'side edit'").run("switch main");
    r.write("a.txt", "one\nmain\n").run("commit -a -m 'main edit'");
    r.run("merge side").run("whereami");
    r.write("a.txt", "one\nboth\n").run("add a.txt").run("whereami").run("merge --abort");
    r.run("rebase side").run("whereami").run("rebase --abort");
    r.run("bisect start HEAD HEAD~3").run("whereami");
    assert_snapshot!(r.finish());
}
