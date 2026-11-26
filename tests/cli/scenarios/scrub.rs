use super::three_commits;
use insta::assert_snapshot;

#[test]
fn scrub_removes_a_secret_from_history_and_purges_it() {
    let mut r = three_commits();
    r.write("secrets.env", "API_KEY=hunter2\n").run("add secrets.env").run("commit -m 'Add config'");
    r.write("a.txt", "one\ntwo\nthree\n").run("commit -a -m 'More work'");
    r.write("secrets.env", "API_KEY=hunter3\n").run("commit -a -m 'Rotate key'");
    r.run("tag -a v1 -m 'Release 1'");
    r.run("rev-parse HEAD:secrets.env");
    r.run("scrub secrets.env --dry-run").run("log --oneline");
    r.run("scrub secrets.env").run("log --oneline").run("ls-tree -r --name-only v1").run("status --short");
    // A second scrub waits for the backup to go; undo takes the first one back.
    r.run("scrub a.txt").run("undo").run("log --oneline -2").run("scrub --drop-backup");
    // Again, and this time all the way: no copy of the secret is left.
    r.run("scrub secrets.env").run("scrub --drop-backup");
    r.run("reflog expire --expire=now --all").run("gc --prune=now").run("repack -a -d").run("recover").run("undo --list");
    // The secret's last version is still in the working tree: its blob is gone from the repository.
    let rgit = |args: &[&str]| String::from_utf8(std::process::Command::new(env!("CARGO_BIN_EXE_rgit")).args(args).current_dir(r.path()).output().unwrap().stdout).unwrap();
    let blob = rgit(&["hash-object", "secrets.env"]).trim().to_string();
    r.mask(&blob, "<SECRET-BLOB>").run(&format!("cat-file -t {}", blob));
    assert_snapshot!(r.finish());
}

#[test]
fn scrub_refuses_what_it_should() {
    let mut r = three_commits();
    r.run("scrub").run("scrub nothing-here.txt").run("scrub ../outside").run("scrub .git/config").run("scrub --drop-backup");
    assert_snapshot!(r.finish());
}
