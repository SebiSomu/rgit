use super::three_commits;
use insta::assert_snapshot;

#[test]
fn recover_lists_and_restores_lost_work() {
    let mut r = three_commits();
    // Nothing lost yet.
    r.run("recover");
    // Two commits reset away, an amended commit, a deleted branch, and a commit made with
    // plumbing that nothing ever pointed at.
    r.write("a.txt", "four\n").run("commit -a -m 'fourth commit'");
    r.write("a.txt", "five\n").run("commit -a -m 'fifth commit'");
    r.run("reset --hard HEAD~2");
    r.write("b.txt", "typo\n").run("commit -a -m 'fix tpyo'").run("commit --amend -m 'fix typo'");
    r.run("switch -c topic").write("topic.txt", "t\n").run("add topic.txt").run("commit -m 'topic work'");
    r.run("switch main").run("branch -D topic");
    r.run("commit-tree HEAD^{tree} -m 'made by hand'");
    r.run("recover");
    // The deleted branch back under its name; the hand-made commit by number; the amended
    // commit by its hash. Each gets a branch (by default `recovered-<commit>`).
    r.run("recover 2 topic").run("log --oneline -1 topic");
    r.run("recover 1");
    r.run("recover");
    let amended = r.read(".git/logs/HEAD").lines().find(|l| l.contains("commit (amend)")).unwrap().split(' ').next().unwrap().to_string();
    r.run(&format!("recover {}", &amended[..8]));
    r.run("recover");
    // Mistakes.
    r.run("recover 9").run("recover ffffffff").run("recover 1 topic");
    assert_snapshot!(r.finish());
}

#[test]
fn recover_puts_a_dropped_stash_back() {
    let mut r = three_commits();
    r.write("src/lib.txt", "half done\n").run("stash push -m 'parser idea'").run("stash drop");
    r.run("stash list").run("recover");
    // Back on the stash list as stash@{0}, so it pops as usual; then nothing is lost.
    r.run("recover 1").run("stash list").run("stash pop").run("status --short").run("recover");
    assert_snapshot!(r.finish());
}
