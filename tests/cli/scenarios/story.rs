//! rgit-only `story`: a file's or directory's history in plain words.
use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn story_of_a_file_with_rename_and_rewrite() {
    let mut r = Repo::init();
    r.set_env("GIT_AUTHOR_NAME", "Ana");
    r.write("parse.rs", "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\n").run("add parse.rs").run("commit -m 'Add parser'");
    r.write("parse.rs", "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\nfn g() {}\n").run("commit -a -m 'Add g'");
    r.set_env("GIT_AUTHOR_NAME", "Bob");
    r.run("mv parse.rs parser.rs").run("commit -m 'Rename parser'");
    r.write("parser.rs", "fn tokens() {}\nfn parse() {}\nfn c() {}\nfn x() {}\nfn y() {}\nfn z() {}\nfn g() {}\n").run("commit -a -m 'Rewrite tokenizer'");
    r.set_env("GIT_AUTHOR_NAME", "Ana");
    r.write("parser.rs", "fn tokens() {}\nfn parse() {}\nfn c() {}\nfn x() {}\nfn y() {}\nfn z() {}\nfn g2() {}\n").run("commit -a -m 'Fix g'");
    r.write("other.txt", "o\n").run("add other.txt").run("commit -m 'Unrelated'");
    r.run("story parser.rs").run("story parser.rs -n 2");
    // blame follows the rename too: "fn c() {}" is still Ana's, from "Add parser".
    r.run("blame -s parser.rs");
    assert_snapshot!(r.finish());
}

#[test]
fn story_of_a_directory_a_deleted_file_and_errors() {
    let mut r = three_commits();
    r.write("src/main.txt", "main\n").run("add src/main.txt").run("commit -m 'Add main'");
    r.run("rm b.txt").run("commit -m 'Drop b'");
    r.run("story src/").run("story b.txt").run("story nothing.txt");
    assert_snapshot!(r.finish());
}
