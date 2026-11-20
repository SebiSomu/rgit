//! Local-path remotes (no network): clone, fetch, push, pull, remote.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn clone_fetch_push_pull() {
    let mut r = three_commits();
    r.run_in("..", "clone repo clone").run_in("../clone", "log --oneline").run_in("../clone", "remote -v").run_in("../clone", "branch -a");
    r.write("upstream.txt", "u\n").run("add upstream.txt").run("commit -m 'upstream change'");
    r.run_in("../clone", "fetch").run_in("../clone", "status").run_in("../clone", "pull").run_in("../clone", "log --oneline -1");
    r.write("../clone/c.txt", "from clone\n");
    r.run_in("../clone", "add c.txt").run_in("../clone", "commit -m 'clone change'");
    r.run("switch -c other");
    r.run_in("../clone", "push").run_in("../clone", "push origin main");
    r.run("switch main").run("log --oneline -1");
    r.run_in("../clone", "remote show origin").run_in("../clone", "remote rename origin up").run_in("../clone", "branch -a").run_in("../clone", "rev-parse --verify -q up");
    assert_snapshot!(r.finish());
}

#[test]
fn remote_management() {
    let mut r = three_commits();
    r.run("remote").run("remote add origin ../elsewhere").run("remote add origin x").run("remote -v").run("remote get-url origin");
    r.run("remote set-url origin ../moved").run("remote rename origin upstream").run("remote -v").run("remote remove upstream").run("remote");
    assert_snapshot!(r.finish());
}
