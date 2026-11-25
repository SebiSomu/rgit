//! rgit-only `ignore`: append patterns to .gitignore and untrack what they now ignore.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn ignore_appends_and_untracks() {
    let mut r = three_commits();
    r.write("build/out.o", "o\n").write("debug.log", "log\n").run("add build debug.log").run("commit -m 'oops, build output'");
    r.write("notes.log", "n\n");
    // A new .gitignore: created, staged with the removals; a typo is flagged; repeats are skipped.
    r.run("ignore -n '*.log' build/").run("ignore '*.log' build/ '*.lgo' '*.log'").run("status --short").run("diff --cached .gitignore");
    r.run("commit -m 'Ignore logs and build output'").run("ls-files").run("status --short").run("ls-files -o -i --exclude-standard");
    // Already there; a `!` rule; a .gitignore with unstaged edits isn't staged.
    r.write(".gitignore", "*.log\nbuild/\n*.lgo\n# mine\n").run("ignore '*.log' '!keep.log' --keep-tracked").run("status --short").run("undo").run("status --short");
    assert_snapshot!(r.finish());
}

#[test]
fn ignore_global_and_info_exclude() {
    let mut r = three_commits();
    r.write("scratch.tmp", "t\n").write("local.txt", "l\n");
    r.run("ignore --global '*.tmp'").run("status --short").run("check-ignore -v scratch.tmp");
    // .git/info/exclude is honored too (and rgit's ignore rules are shared with git's files).
    r.write(".git/info/exclude", "local.txt\n").run("status --short").run("check-ignore -v local.txt").run("ls-files -o --exclude-standard");
    r.run("ignore '#x'");
    assert_snapshot!(r.finish());
}
