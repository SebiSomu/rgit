//! rgit-only `split`: one commit into several, by hunk.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn split_commit_by_hunk() {
    let mut r = three_commits();
    r.write("a.txt", "ONE\ntwo\nthree\n").write("b.txt", "bee!\n").write("c.txt", "sea\n");
    r.run("add .").run("commit -m 'mixed bag' -m 'Several unrelated changes.'");
    r.write("src/lib.txt", "lib v3\n").run("commit -a -m 'on top'");
    // An unstaged edit: split never touches the files.
    r.write("src/lib.txt", "lib v3 unstaged\n");
    // Commit 1: a.txt line 1 and the new c.txt; commit 2: the added line (default message);
    // commit 3: b.txt, alone, gets the original message.
    r.run_stdin("split HEAD~1", "y\nn\nn\ny\nUppercase one, add c\ny\nn\n\n\n");
    r.run("log --oneline").run("show HEAD~3 --stat").run("show HEAD~2 --stat").run("show HEAD~1").run("status --short");
    assert_snapshot!(r.finish());
}

#[test]
fn split_refusals_retry_cancel_and_undo() {
    let mut r = three_commits();
    r.run("split HEAD~1").run("split HEAD~2");
    // Quitting changes nothing.
    r.run_stdin("split", "n\nq\n").run("log --oneline -1");
    // Taking every hunk for the first commit asks again.
    r.run_stdin("split", "y\ny\ny\nn\nAdd b.txt\n\n").run("log --oneline");
    r.run("undo").run("log --oneline");
    // Commits a remote-tracking branch has are refused without --force.
    r.run_in("..", "clone repo clone").run_in("../clone", "split");
    assert_snapshot!(r.finish());
}
