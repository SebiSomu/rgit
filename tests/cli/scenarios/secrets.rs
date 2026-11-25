//! rgit-only `secrets`: obvious secrets in staged changes, tracked files or history.
//!
//! The fake secrets are put together at run time (and removed before the final state is
//! printed), so this file and the snapshots never hold one that a secret scanner would flag.
use super::three_commits;
use insta::assert_snapshot;

fn fake_aws() -> String {
    ["AK", "IA", &"Q".repeat(16)].concat()
}

fn fake_github() -> String {
    ["gh", "p_", &"a1".repeat(18)].concat()
}

#[test]
fn secrets_in_staged_changes() {
    let mut r = three_commits();
    r.run("secrets");
    r.write("config.py", &format!("DEBUG = True\nAWS_KEY = \"{}\"\nDB = \"postgres://app:hunter22@db:5432/app\"\n", fake_aws()));
    r.write(".env", "TOKEN=x\n").write(".env.example", "TOKEN=\n");
    r.write("a.txt", &format!("one\ntwo\nmy_token = \"{}\"\nok = \"{}\"  # rgit:allow-secret\n", fake_github(), fake_github()));
    r.run("add .").run("secrets").run("secrets -q");
    // Only the lines the staged changes add count: committed, then staged again unchanged.
    r.run("commit -m 'oops' -q").run("secrets").run("secrets --all-files");
    r.write("a.txt", "one\ntwo\n").run("rm config.py .env").run("commit -a -m 'clean up' -q");
    assert_snapshot!(r.finish());
}

#[test]
fn secrets_in_history() {
    let mut r = three_commits();
    r.write("deploy.sh", &format!("export KEY={}\n", fake_aws())).run("add deploy.sh").run("commit -m 'Deploy script'");
    r.run("rm deploy.sh").run("commit -m 'Remove it'");
    r.run("secrets").run("secrets --history").run("secrets --history HEAD~1..");
    assert_snapshot!(r.finish());
}
