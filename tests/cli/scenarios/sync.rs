//! rgit-only `sync`: fetch + fast-forward / rebase / merge onto the upstream (+ push), with a plan.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn sync_fast_forward_rebase_merge_push() {
    let mut r = three_commits();
    r.run_in("..", "clone repo clone");
    r.run_in("../clone", "sync");
    // Behind only: fast-forward, local edits stashed around it.
    r.write("up.txt", "u1\n").run("add up.txt").run("commit -m 'upstream one'");
    r.write("../clone/a.txt", "one\ntwo\nlocal edit\n");
    r.run_in("../clone", "sync").run_in("../clone", "status --short");
    // Diverged: dry run, then rebase (the default) and push.
    r.write("up.txt", "u2\n").run("commit -a -m 'upstream two'");
    r.write("../clone/c.txt", "c\n").run_in("../clone", "add c.txt").run_in("../clone", "commit -m 'clone work'");
    r.run_in("../clone", "sync --dry-run").run_in("../clone", "sync --push").run_in("../clone", "log --oneline -3");
    r.run("branch -v");
    // (The push landed on the checked-out branch here; bring its working tree along.)
    // Ahead only, and --merge on a diverged branch.
    r.write("../clone/d.txt", "d\n").run_in("../clone", "add d.txt").run_in("../clone", "commit -m 'clone more'").run_in("../clone", "sync --no-fetch");
    r.run("reset --hard").write("up.txt", "u3\n").run("commit -a -m 'upstream three'");
    r.run_in("../clone", "sync --merge").run_in("../clone", "log --oneline -1");
    assert_snapshot!(r.finish());
}

#[test]
fn sync_conflict_undo_and_publish() {
    let mut r = three_commits();
    r.run_in("..", "clone repo clone");
    r.write("a.txt", "ONE\ntwo\n").run("commit -a -m 'upstream edit'");
    r.write("../clone/a.txt", "uno\ntwo\n").run_in("../clone", "commit -a -m 'clone edit'");
    // The rebase conflicts: stop with a note, `rgit undo` puts everything back.
    r.run_in("../clone", "sync").run_in("../clone", "status --short").run_in("../clone", "undo").run_in("../clone", "log --oneline -2").run_in("../clone", "status --short");
    // A new branch: nothing to sync until --push publishes it.
    r.run_in("../clone", "switch -c topic").run_in("../clone", "sync").run_in("../clone", "sync --push").run_in("../clone", "sync");
    r.run("branch");
    r.run("switch --detach HEAD").run("sync");
    assert_snapshot!(r.finish());
}
