//! Scenario groups. Each test drives rgit through a small story and snapshots the transcript.
mod basics;
mod branching;
mod history;
mod ls_remote;
mod plumbing;
mod narration;
mod errors;
mod mail;
mod maintenance;
mod remotes;
mod misc;
mod oops;
mod open;
mod undo;
mod wip;
mod fixup;
mod sync;
mod absorb;
mod backup;
mod changelog;
mod compare;
mod contributors;
mod doctor;
mod hotspots;
mod ignore;
mod move_commit;
mod secrets;
mod split;
mod stats;
mod story;
mod timeline;
mod tip;
mod whereami;

use crate::harness::Repo;

/// A repository with three commits on `main`: a.txt edits plus a file in a subdirectory.
pub fn three_commits() -> Repo {
    let mut r = Repo::init();
    r.write("a.txt", "one\n").write("src/lib.txt", "lib v1\n");
    r.run("add .").run("commit -m 'first commit'");
    r.write("a.txt", "one\ntwo\n");
    r.run("commit -a -m 'second commit'");
    r.write("src/lib.txt", "lib v2\n").write("b.txt", "bee\n");
    r.run("add .").run("commit -m 'third commit' -m 'With a body paragraph.'");
    r
}

/// `main` and `feature` diverged from a common base, touching different files.
pub fn diverged() -> Repo {
    let mut r = Repo::init();
    r.write("base.txt", "base\n");
    r.run("add base.txt").run("commit -m base");
    r.run("branch feature");
    r.write("main.txt", "on main\n");
    r.run("add main.txt").run("commit -m 'main work'");
    r.run("switch feature");
    r.write("feature.txt", "on feature\n");
    r.run("add feature.txt").run("commit -m 'feature work'");
    r.write("feature.txt", "on feature\nmore\n");
    r.run("commit -a -m 'more feature work'");
    r.run("switch main");
    r
}

/// `main` and `topic` both edited line 2 of conflict.txt.
pub fn conflicting() -> Repo {
    let mut r = Repo::init();
    r.write("conflict.txt", "line1\nline2\nline3\n");
    r.run("add conflict.txt").run("commit -m base");
    r.run("branch topic");
    r.write("conflict.txt", "line1\nmain change\nline3\n");
    r.run("commit -a -m 'main edit'");
    r.run("switch topic");
    r.write("conflict.txt", "line1\ntopic change\nline3\n");
    r.run("commit -a -m 'topic edit'");
    r.run("switch main");
    r
}
