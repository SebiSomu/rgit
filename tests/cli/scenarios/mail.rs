//! Patch and mail workflows.
use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn format_patch_and_am() {
    let mut r = three_commits();
    r.run("format-patch -2 --stdout").run("format-patch -1 -o patches").run("format-patch --root -n --stdout HEAD~1");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_rgit")).args(["format-patch", "-2", "--stdout"]).current_dir(r.path()).output().unwrap();
    let mbox = String::from_utf8_lossy(&out.stdout).to_string();
    r.run("reset --hard HEAD~2").run("clean -f -d");
    r.run_stdin("am", &mbox).run("log --format='%s | %an'").run("am --abort");
    assert_snapshot!(r.finish());
}

#[test]
fn apply_patches() {
    let mut r = three_commits();
    let patch = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,3 @@\n one\n two\n+three\n";
    r.write("p.diff", patch);
    r.run("apply --check p.diff").run("apply --stat p.diff").run("apply --numstat p.diff").run("apply p.diff").run("diff");
    r.run("apply -R p.diff").run("apply --cached p.diff").run("diff --cached").run("apply p.diff");
    r.write("bad.diff", "diff --git a/zzz b/zzz\n--- a/zzz\n+++ b/zzz\n@@ -1 +1 @@\n-nope\n+x\n").run("apply bad.diff");
    assert_snapshot!(r.finish());
}

#[test]
fn mailinfo_mailsplit_trailers() {
    let mut r = Repo::init();
    let mail = "From 1234567890abcdef1234567890abcdef12345678 Mon Sep 17 00:00:00 2001\nFrom: =?UTF-8?q?J=C3=B6rg?= <jorg@example.com>\nDate: Tue, 14 Nov 2023 22:13:20 +0000\nSubject: [PATCH 1/2] Fix the thing\n\nBody text.\n\nSigned-off-by: J <j@example.com>\n---\n a | 1 +\n\ndiff --git a/a b/a\n";
    r.write("msg", "").write("patch", "");
    r.run_stdin("mailinfo msg patch", mail).run_stdin("mailinfo -k msg patch", mail);
    r.write("box.mbox", &format!("{}{}", mail, mail.replace("1/2", "2/2"))).run("mailsplit -oout box.mbox");
    r.run_stdin("interpret-trailers --trailer 'Reviewed-by: R <r@example.com>'", "Subject\n\nBody\n");
    r.run_stdin("interpret-trailers --parse", "Subject\n\nBody\n\nSigned-off-by: A <a@x>\nAcked-by: B\n");
    r.run_stdin("check-mailmap 'Some One <some@example.com>'", "");
    r.run_stdin("patch-id", "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-x\n+y\n");
    assert_snapshot!(r.finish());
}
