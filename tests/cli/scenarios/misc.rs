//! rgit-specific commands and the remaining porcelain.
use super::{diverged, three_commits};
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn banana_split() {
    let mut r = Repo::init();
    r.write("x", "x\n").run("add x").run("commit -m base");
    r.write("docs/a.md", "a\n").write("src/b.rs", "b\n").write("README", "r\n").run("add .").run("commit -m 'mixed commit'");
    r.run("banana-split HEAD").run("log --stat --format=%s");
    assert_snapshot!(r.finish());
}

#[test]
fn notes_and_replace() {
    let mut r = three_commits();
    r.run("notes add -m 'a note' HEAD").run("notes show HEAD").run("notes list").run("log -1 --format='%s%n%N'");
    r.run("notes append -m 'more' HEAD").run("notes show").run("notes remove HEAD").run("notes list");
    r.run("replace HEAD~1 HEAD~2").run("replace -l").run("log --format=%s").run("replace -d HEAD~1");
    assert_snapshot!(r.finish());
}

#[test]
fn sparse_checkout() {
    let mut r = three_commits();
    r.run("sparse-checkout init --cone").run("sparse-checkout set src").run("sparse-checkout list").run("status");
    r.run("sparse-checkout disable");
    assert_snapshot!(r.finish());
}

#[test]
fn rerere_replay_fmt_merge_msg() {
    let mut r = diverged();
    r.run("rerere status").run("replay --onto main main..feature");
    r.run_stdin("fmt-merge-msg", "0123456789012345678901234567890123456789\t\tbranch 'feature' of ../x\n");
    r.run("repo info").run("version").run("hook run pre-commit --ignore-missing");
    assert_snapshot!(r.finish());
}

#[test]
fn unknown_command_suggestions() {
    let mut r = Repo::new();
    for cmd in ["comit", "stauts", "stat", "psuh", "chekout", "nosuchcommand", "--plain brnach", "help rebsae"] {
        r.run(cmd);
    }
    assert_snapshot!(r.finish());
}

#[test]
fn help_output() {
    let mut r = Repo::new();
    r.run("help").run("help commit").run("help -a").run("--help").run("commit --help");
    assert_snapshot!(r.finish());
}
