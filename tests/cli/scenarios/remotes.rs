//! Local-path remotes (no network): clone, fetch, push, pull, remote.
use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn clone_fetch_push_pull() {
    let mut r = three_commits();
    r.run_in("..", "clone repo clone").run_in("../clone", "log --oneline").run_in("../clone", "remote -v").run_in("../clone", "branch -a");
    r.write("upstream.txt", "u\n").run("add upstream.txt").run("commit -m 'upstream change'");
    r.run_in("../clone", "fetch").run_in("../clone", "status").run_in("../clone", "pull").run_in("../clone", "log --oneline -1");
    r.write("../clone/c.txt", "from clone\n");
    r.run_in("../clone", "add c.txt").run_in("../clone", "commit -m 'clone change'");
    r.run("switch -c other");
    r.run_in("../clone", "push").run_in("../clone", "push origin main");
    r.run("switch main").run("log --oneline -1");
    r.run_in("../clone", "remote show origin").run_in("../clone", "remote rename origin up").run_in("../clone", "branch -a").run_in("../clone", "rev-parse --verify -q up");
    assert_snapshot!(r.finish());
}

#[test]
fn remote_management() {
    let mut r = three_commits();
    r.run("remote").run("remote add origin ../elsewhere").run("remote add origin x").run("remote -v").run("remote get-url origin");
    r.run("remote set-url origin ../moved").run("remote rename origin upstream").run("remote -v").run("remote remove upstream").run("remote");
    assert_snapshot!(r.finish());
}

#[test]
fn branch_rename_push() {
    let mut r = three_commits();
    r.run("branch feature").run_in("..", "clone repo clone");
    r.run_in("../clone", "switch feature");
    // Rename here and on the remote: push the new name, track it, delete the old one.
    r.run_in("../clone", "branch -m feature feat-x --push").run_in("../clone", "branch -a").run_in("../clone", "config branch.feat-x.merge");
    r.run("branch");
    // A branch without an upstream needs the remote named; then there is nothing to delete.
    r.run_in("../clone", "branch solo").run_in("../clone", "branch -m solo duo --push").run_in("../clone", "branch -m solo duo --push=origin");
    r.run("branch");
    r.run_in("../clone", "branch -m duo trio --push=nope").run_in("../clone", "branch --push").run_in("../clone", "branch -c duo trio --push");
    // Undo only renames it back locally.
    r.run_in("../clone", "undo").run_in("../clone", "branch");
    assert_snapshot!(r.finish());
}

#[test]
fn clone_and_fetch_from_bundle() {
    let mut r = three_commits();
    r.run("branch feature").run("bundle create ../full.bundle --all");
    r.run_in("..", "clone full.bundle").run_in("../full", "branch -a").run_in("../full", "remote -v").run_in("../full", "status");
    r.run_in("..", "clone -b feature full.bundle feat").run_in("../feat", "branch");
    // An incremental bundle needs its prerequisite commits: a fresh clone can't use it...
    r.write("d.txt", "dee\n").run("add d.txt").run("commit -m 'fourth commit'").run("bundle create ../incr.bundle main~1..main");
    r.run_in("..", "clone incr.bundle incr");
    // ...but a clone that already has them can fetch from it.
    r.run_in("../full", "remote set-url origin ../incr.bundle").run_in("../full", "fetch").run_in("../full", "log --oneline -2 origin/main");
    assert_snapshot!(r.finish());
}

#[test]
fn credential_helpers() {
    let mut r = Repo::new();
    // No helper and no terminal: `fill` fails like git.
    r.set_env("GIT_TERMINAL_PROMPT", "0");
    r.run_stdin("credential fill", "protocol=https\nhost=github.com\n\n");
    r.run("config --global credential.helper store");
    r.run_stdin("credential approve", "protocol=https\nhost=github.com\nusername=sebi\npassword=tok/en%1\n\n");
    r.run_stdin("credential approve", "url=https://me@gitlab.com\npassword=p@ss\n\n");
    // Saving the same user's login again replaces it; the newest login comes first.
    r.run_stdin("credential approve", "protocol=https\nhost=github.com\nusername=sebi\npassword=newer\n\n");
    r.show_file("../home/.git-credentials");
    r.run_stdin("credential fill", "protocol=https\nhost=github.com\n\n");
    r.run_stdin("credential fill", "protocol=https\nhost=gitlab.com\nusername=someone-else\n\n");
    r.run_stdin("credential reject", "protocol=https\nhost=github.com\n\n");
    r.show_file("../home/.git-credentials");
    // The helper itself, with its own file.
    r.run_stdin("credential-store --file creds store", "protocol=https\nhost=example.com\nusername=a\npassword=b\n\n");
    r.run_stdin("credential-store --file creds get", "protocol=https\nhost=example.com\n\n");
    r.run_stdin("credential-store --file creds erase", "protocol=https\nhost=example.com\nusername=a\n\n");
    r.show_file("creds");
    // Errors.
    r.run_stdin("credential fill", "host=example.com\n\n").run_stdin("credential fill", "not a line\n");
    r.run("credential").run("credential-store --bogus get");
    assert_snapshot!(r.finish());
}
