//! rgit-only `timeline`: a day-by-day digest of the repository.
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn timeline_days_branches_merges_and_gaps() {
    let mut r = Repo::init();
    let at = |r: &mut Repo, who: &str, date: &str| {
        r.set_env("GIT_AUTHOR_NAME", who).set_env("GIT_AUTHOR_DATE", date).set_env("GIT_COMMITTER_DATE", date);
    };
    at(&mut r, "Ana", "2023-11-06T10:00:00 +0200");
    r.write("a.txt", "a\n").run("add a.txt").run("commit -m 'Start'");
    at(&mut r, "Ana", "2023-11-09T09:00:00 +0200");
    r.write("a.txt", "a\nb\n").run("commit -a -m 'Add b'");
    r.run("switch -c feature/x");
    at(&mut r, "Bob", "2023-11-09T15:00:00 +0200");
    r.write("x.txt", "x\n").run("add x.txt").run("commit -m 'Start x'");
    at(&mut r, "Bob", "2023-11-09T16:00:00 +0200");
    r.write("x.txt", "x\ny\n").run("commit -a -m 'More x'");
    at(&mut r, "Ana", "2023-11-09T17:00:00 +0200");
    r.write("x.txt", "x\ny\nz\n").run("commit -a -m 'Review x'");
    at(&mut r, "Bob", "2023-11-09T18:00:00 +0200");
    r.write("x.txt", "x\ny\nz\nw\n").run("commit -a -m 'Polish x'");
    r.run("switch main");
    // 23:30 at -0500 is still Friday for Ana, though Saturday in UTC.
    at(&mut r, "Ana", "2023-11-10T23:30:00 -0500");
    r.write("a.txt", "a\nb\nc\n").run("commit -a -m 'Add c'");
    at(&mut r, "Ana", "2023-11-13T11:00:00 +0200");
    r.run("merge feature/x --no-ff -m \"Merge branch 'feature/x'\"");
    r.run("timeline").run("timeline -n 2").run("timeline --current --since 2023-11-10").run("timeline --since 2030-01-01");
    assert_snapshot!(r.finish());
}
