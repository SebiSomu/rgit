use super::three_commits;
use insta::assert_snapshot;

#[test]
fn wip_save_list_restore() {
    let mut r = three_commits();
    r.write(".gitignore", "*.log\n").run("add .gitignore").run("commit -m ignore-logs");
    // Staged, unstaged and untracked changes all go into one save; ignored files stay put.
    r.write("a.txt", "staged edit\n").run("add a.txt").write("a.txt", "staged edit\nplus unstaged\n");
    r.write("notes/todo.md", "parser\n").write("build.log", "noise\n");
    r.run("wip halfway through parser").run("status --short").run("wip");
    r.write("b.txt", "second save\n").run("save").run("wip --list");
    // Restore the older one: staged stays staged, untracked stays untracked; the stack relinks.
    r.run("wip --restore 1").run("status --short").run("wip --list");
    // A dirty tree refuses the next restore.
    r.run("wip --restore");
    assert_snapshot!(r.finish());
}

#[test]
fn wip_restore_after_head_moved_conflict_and_undo() {
    let mut r = three_commits();
    r.write("a.txt", "wip version\n").run("wip");
    r.write("a.txt", "committed version\n").run("commit -a -m moved-on");
    r.run("wip --restore").run("status --short").run("wip --list");
    r.run("reset --hard").run("wip --drop").run("wip --list").run("undo").run("wip --list");
    r.run("switch --detach HEAD").run("wip").run("wip --list --all").run("switch main");
    r.run("wip --drop 3").run("switch -c empty").run("wip --list").run("wip --restore");
    assert_snapshot!(r.finish());
}
