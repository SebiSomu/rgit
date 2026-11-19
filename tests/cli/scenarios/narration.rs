//! rgit's own narration layer (`--explain`, `--teach`, `--plain`, RGIT_VERBOSITY, rgit.verbosity).
use super::{conflicting, diverged};
use crate::harness::Repo;
use insta::assert_snapshot;

fn narrated_story(level: &str) -> String {
    let mut r = Repo::init();
    r.set_env("RGIT_VERBOSITY", level);
    r.write("a.txt", "a\n").write(".env", "SECRET=1\n");
    r.run("add .").run("commit -m first");
    r.write("a.txt", "a2\n").run("add a.txt").run("commit -m second");
    r.run("branch side").run("switch side").run("switch main");
    r.run("reset --hard HEAD~1").run("status");
    r.finish()
}

#[test]
fn verbosity_quiet() {
    assert_snapshot!(narrated_story("quiet"));
}

#[test]
fn verbosity_normal() {
    assert_snapshot!(narrated_story("normal"));
}

#[test]
fn verbosity_explain() {
    assert_snapshot!(narrated_story("explain"));
}

#[test]
fn verbosity_teach() {
    assert_snapshot!(narrated_story("teach"));
}

#[test]
fn global_flags() {
    let mut r = diverged();
    r.run("--explain merge feature -m m").run("--teach log --oneline -1").run("--plain status").run("--explain --plain status");
    assert_snapshot!(r.finish());
}

#[test]
fn config_verbosity_and_conflict_narration() {
    let mut r = conflicting();
    r.run("config rgit.verbosity explain").run("merge topic").run("status").run("conflicts").run("merge --abort");
    assert_snapshot!(r.finish());
}
