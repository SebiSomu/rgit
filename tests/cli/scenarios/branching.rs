use super::{conflicting, diverged, three_commits};
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn branch_create_list_delete() {
    let mut r = three_commits();
    r.run("branch").run("branch feature").run("branch feature/nested HEAD~1").run("branch -v").run("branch --list 'feat*'");
    r.run("branch -m feature renamed").run("branch");
    r.run("branch -d feature/nested").run("branch -d main");
    r.run("branch 'bad..name'").run("branch renamed").run("branch -D renamed").run("branch");
    assert_snapshot!(r.finish());
}

#[test]
fn switch_and_checkout() {
    let mut r = diverged();
    r.run("switch feature").run("status").run("switch -c new-branch").run("switch -");
    r.run("checkout main").run("checkout -b via-checkout").run("checkout HEAD~1").run("status");
    r.run("switch --detach main").run("switch main");
    r.write("main.txt", "dirty\n").run("switch feature");
    r.run("checkout -- main.txt").run("switch feature").run("branch");
    assert_snapshot!(r.finish());
}

#[test]
fn merge_fast_forward() {
    let mut r = three_commits();
    r.run("switch -c ahead");
    r.write("ff.txt", "ff\n").run("add ff.txt").run("commit -m ahead");
    r.run("switch main").run("merge ahead").run("merge ahead").run("log --oneline");
    assert_snapshot!(r.finish());
}

#[test]
fn merge_three_way_clean() {
    let mut r = diverged();
    r.run("merge feature -m 'Merge feature'").run("log --oneline --graph").run("merge-base main feature");
    assert_snapshot!(r.finish());
}

#[test]
fn merge_no_ff_and_squash() {
    let mut r = diverged();
    r.run("switch -c squashed");
    r.run("merge --squash feature").run("status --short").run("commit -m squashed");
    r.run("switch main").run("merge --no-ff feature -m 'no ff'").run("log --oneline");
    assert_snapshot!(r.finish());
}

#[test]
fn merge_conflict_resolve() {
    let mut r = conflicting();
    r.run("merge topic").run("status").checkpoint("conflicted");
    r.run("diff");
    r.run("commit -m 'try commit while conflicted'");
    r.write("conflict.txt", "line1\nresolved\nline3\n").run("add conflict.txt").run("status");
    r.run("commit -m 'Merge topic'").run("log --oneline");
    assert_snapshot!(r.finish());
}

#[test]
fn merge_conflict_abort() {
    let mut r = conflicting();
    r.run("merge topic").run("merge --abort").run("status");
    assert_snapshot!(r.finish());
}

#[test]
fn rebase_linear() {
    let mut r = diverged();
    r.run("switch feature").run("rebase main").run("log --format='%s'").run("rebase main");
    assert_snapshot!(r.finish());
}

#[test]
fn rebase_conflict_continue_and_abort() {
    let mut r = conflicting();
    r.run("switch topic").run("rebase main").run("status").checkpoint("rebase stopped");
    r.run("rebase --abort").run("log --format='%s'");
    r.run("rebase main");
    r.write("conflict.txt", "line1\nboth\nline3\n").run("add conflict.txt").run("rebase --continue");
    r.run("log --format='%s'");
    assert_snapshot!(r.finish());
}

#[test]
fn rebase_onto() {
    let mut r = diverged();
    r.run("branch base-two main~1").run("switch feature").run("rebase --onto base-two main~1 feature").run("log --format='%s'");
    assert_snapshot!(r.finish());
}

#[test]
fn cherry_pick_and_revert() {
    let mut r = diverged();
    r.run("cherry-pick feature").run("log --format='%s'");
    r.run("cherry-pick -x feature~1").run("log -1 --format=%B");
    r.run("revert HEAD --no-edit").run("log --format='%s'");
    r.run("cherry-pick nonexistent");
    assert_snapshot!(r.finish());
}

#[test]
fn cherry_pick_conflict() {
    let mut r = conflicting();
    r.run("cherry-pick topic").run("status").run("cherry-pick --abort").run("status --short");
    r.run("cherry-pick topic");
    r.write("conflict.txt", "picked\n").run("add conflict.txt").run("cherry-pick --continue");
    r.run("log --format='%s'");
    assert_snapshot!(r.finish());
}

#[test]
fn stash_flow() {
    let mut r = three_commits();
    r.run("stash list").run("stash");
    r.write("a.txt", "wip 1\n").run("stash").run("stash list");
    r.write("b.txt", "wip 2\n").write("untracked.txt", "u\n").run("stash push -u -m 'second wip'").run("stash list");
    r.run("stash show").run("stash show -p stash@{1}");
    r.run("stash apply stash@{1}").run("status --short").run("reset --hard");
    r.run("stash pop").run("status --short");
    r.run("stash drop").run("stash list").run("stash drop");
    r.write("a.txt", "x\n").run("stash").run("stash clear").run("stash list");
    assert_snapshot!(r.finish());
}

#[test]
fn tags_and_describe() {
    let mut r = three_commits();
    r.run("describe").run("describe --always");
    r.run("tag v1.0 HEAD~2").run("tag -a v2.0 -m 'release two' HEAD~1").run("tag").run("tag -n");
    r.run("describe").run("describe --tags").run("describe --long --tags").run("describe --exact-match HEAD~1");
    r.run("tag v1.0").run("tag -f v1.0").run("tag -d v2.0").run("tag -l 'v*'");
    r.run("show v1.0 --stat");
    assert_snapshot!(r.finish());
}

#[test]
fn bisect_session() {
    let mut r = Repo::init();
    for i in 1..=6 {
        r.write("n.txt", &format!("{}\n", i));
        if i == 1 { r.run("add n.txt"); }
        r.run(&format!("commit -a -m 'commit {}'", i));
    }
    r.run("bisect start").run("bisect bad").run("bisect good HEAD~5");
    r.run("bisect good").run("bisect bad").run("bisect log").run("bisect reset").run("status");
    assert_snapshot!(r.finish());
}

#[test]
fn worktree_basic() {
    let mut r = three_commits();
    r.run("worktree add ../wt -b wt-branch").run("worktree list --porcelain");
    r.write("../wt/wt.txt", "from worktree\n");
    r.run_in("../wt", "add wt.txt").run_in("../wt", "commit -m 'in worktree'").run_in("../wt", "status");
    r.run("log --all --format='%s'").run("worktree remove ../wt").run("worktree list");
    assert_snapshot!(r.finish());
}
