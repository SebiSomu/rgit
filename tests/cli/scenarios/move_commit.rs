//! rgit-only `move-commit`: cherry-pick commits onto another branch and drop them here, without switching.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn move_commit_copy_create_and_undo() {
    let mut r = three_commits();
    r.run("branch side HEAD~2");
    r.write("c.txt", "sea\n").run("add c.txt").run("commit -m 'fourth commit'");
    // An unstaged edit to a file the move doesn't change stays put.
    r.write("a.txt", "one\ntwo\nunstaged\n");
    r.run("move-commit HEAD~1 side").run("log --oneline").run("log --oneline side").run("show side --stat").run("status --short");
    // -c: a new branch at the parent of the commit moved.
    r.run("move-commit HEAD topic -c").run("log --oneline --all --graph");
    // --copy: the commit stays here too.
    r.run("move-commit --copy HEAD side").run("log --oneline side").run("log --oneline -1");
    // A range: both commits on top of "first commit" move, oldest first (the unstaged edit
    // is in a file the drop changes, so it has to go first).
    r.run("switch -c two").write("d.txt", "d\n").run("add d.txt").run("commit -m 'fifth commit'");
    r.run("move-commit HEAD~2.. fresh -c").run("restore a.txt").run("move-commit HEAD~2.. fresh -c");
    r.run("log --oneline fresh").run("log --oneline");
    r.run("undo").run("log --oneline topic");
    assert_snapshot!(r.finish());
}

#[test]
fn move_commit_refusals() {
    let mut r = three_commits();
    r.run("branch side HEAD~2");
    r.run("move-commit HEAD nope").run("move-commit HEAD main").run("move-commit HEAD~2 side");
    // "fourth commit" edits the line "second commit" added: it can't stay without it.
    r.write("a.txt", "one\nTWO\n").run("commit -a -m 'fourth commit'");
    r.run("move-commit HEAD~2 side");
    // On 'other' a.txt differs: "fourth commit" doesn't apply there.
    r.run("branch other HEAD~3").run("switch other").write("a.txt", "uno\n").run("commit -a -m 'other edit'").run("switch main");
    r.run("move-commit HEAD other");
    // Uncommitted changes in a file the drop changes.
    r.write("src/lib.txt", "dirty\n").run("move-commit HEAD~1 side").run("restore src/lib.txt");
    // A range, and the pushed check.
    r.run("move-commit HEAD~1.. side").run("log --oneline side");
    r.run_in("..", "clone repo clone").run_in("../clone", "move-commit HEAD~1.. other -c").run_in("../clone", "move-commit --copy HEAD other -c");
    assert_snapshot!(r.finish());
}
