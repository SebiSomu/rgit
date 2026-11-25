//! rgit-only `compare`: where two branches stand against each other, in one answer.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn compare_ahead_behind_diverged_and_same() {
    let mut r = three_commits();
    r.run("branch base").run("switch -c feature");
    r.write("a.txt", "one\ntwo\nfeature\n").run("commit -a -m 'Feature edit'");
    r.write("x.txt", "x\n").run("add x.txt").run("commit -m 'Add x'");
    r.run("compare feature base").run("compare base feature").run("compare base base");
    r.run("switch main").write("a.txt", "one\ntwo\nmain\n").run("commit -a -m 'Main edit'");
    r.write("b.txt", "bee\nmore\n").run("commit -a -m 'Main b'");
    r.run("cherry-pick feature").run("switch feature");
    // From feature: HEAD against main (one argument).
    r.run("compare main").run("compare feature main -n 1");
    r.run("compare nope");
    assert_snapshot!(r.finish());
}

#[test]
fn compare_with_upstream_and_clean_merge() {
    let mut r = three_commits();
    r.run_in("..", "clone repo clone");
    r.write("b.txt", "bee\nupstream\n").run("commit -a -m 'Upstream b'");
    r.write("../clone/c.txt", "c\n").run_in("../clone", "add c.txt").run_in("../clone", "commit -m 'Local c'").run_in("../clone", "fetch");
    // No arguments: HEAD against its upstream; the two sides touch different files.
    r.run_in("../clone", "compare");
    r.run("compare");
    assert_snapshot!(r.finish());
}
