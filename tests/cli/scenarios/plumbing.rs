use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn objects_and_trees() {
    let mut r = Repo::init();
    r.write("f.txt", "plumbing\n").write("d/g.txt", "nested\n");
    r.run("hash-object f.txt").run("hash-object -w f.txt").run("cat-file -p 'HEAD'");
    r.run("add .").run("write-tree").run("ls-files -s").run("ls-files").run("ls-files --stage d");
    r.run("commit -m tree");
    r.run("ls-tree HEAD").run("ls-tree --name-only HEAD").run("ls-tree -r HEAD").run("ls-tree HEAD d");
    r.run("cat-file -t HEAD").run("cat-file -s HEAD").run("cat-file -p HEAD").run("cat-file -p HEAD:d/g.txt").run("cat-file -e HEAD");
    r.run("cat-file -t 0000000000000000000000000000000000000000");
    assert_snapshot!(r.finish());
}

#[test]
fn commit_tree_and_refs() {
    let mut r = three_commits();
    r.run("rev-parse HEAD^{tree}");
    let tree = r.read(".git/refs/heads/main");
    let _ = tree;
    r.run_stdin("commit-tree HEAD^{tree} -p HEAD -m 'manual commit'", "");
    r.run("update-ref refs/heads/manual HEAD~1").run("show-ref refs/heads/manual");
    r.run("update-ref -d refs/heads/manual").run("show-ref refs/heads/manual");
    r.run("symbolic-ref HEAD").run("symbolic-ref HEAD refs/heads/other").run("symbolic-ref --short HEAD").run("symbolic-ref HEAD refs/heads/main");
    r.run("pack-refs --all").run("show-ref");
    r.run("check-ref-format refs/heads/ok").run("check-ref-format 'refs/heads/bad..x'").run("check-ref-format --branch main").run("check-ref-format --normalize 'refs//heads/x'");
    assert_snapshot!(r.finish());
}

#[test]
fn index_plumbing() {
    let mut r = three_commits();
    r.write("new.txt", "n\n");
    r.run("update-index --add new.txt").run("ls-files -s").run("update-index --remove new.txt").run("ls-files");
    r.run("update-index --chmod=+x a.txt").run("ls-files -s a.txt");
    r.run("read-tree HEAD~2").run("ls-files -s").run("read-tree HEAD").run("checkout-index -a -f").run("diff-files").run("diff-index HEAD");
    r.write("a.txt", "x\n").run("diff-files --name-only").run("diff-index --cached HEAD").run("diff-tree HEAD~1 HEAD").run("diff-tree -r --root HEAD~2");
    assert_snapshot!(r.finish());
}

#[test]
fn mktree_mktag_merge_file() {
    let mut r = three_commits();
    r.run("ls-tree HEAD");
    let listing = {
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_rgit")).args(["ls-tree", "HEAD"]).current_dir(r.path()).output().unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    r.run_stdin("mktree", &listing);
    r.write("base", "a\nb\nc\n").write("ours", "a\nB\nc\n").write("theirs", "a\nb\nC\n").write("conf", "a\nX\nc\n");
    r.run("merge-file -p ours base theirs").run("merge-file -p conf base ours").run("merge-file -p --diff3 conf base ours").run("merge-file -p --union conf base ours");
    r.run("merge-tree --write-tree main main~1");
    r.run("stripspace").run_stdin("stripspace", "  a  \n\n\n# c\nb\n").run_stdin("stripspace -s", "keep\n# drop\n").run_stdin("stripspace -c", "x\n\ny\n");
    r.run_stdin("column --mode=column --width=20", "a\nb\nc\nd\ne\nf\n");
    // `var -l` lists platform defaults (editor, shell), so only portable variables are pinned.
    r.run("var GIT_AUTHOR_IDENT").run("var GIT_DEFAULT_BRANCH").run("var GIT_PAGER");
    assert_snapshot!(r.finish());
}

#[test]
fn fast_export_import() {
    let mut r = three_commits();
    r.run("fast-export --all");
    let stream = std::process::Command::new(env!("CARGO_BIN_EXE_rgit")).args(["fast-export", "--all"]).current_dir(r.path()).output().unwrap();
    let stream = String::from_utf8_lossy(&stream.stdout).to_string();
    r.run("init ../imported");
    r.run_stdin("fast-import --quiet", &stream);
    r.run("log --format=%s");
    let mut i = Repo::init();
    i.run_stdin("fast-import", &stream).run("log --all --format='%s'").run("fsck");
    assert_snapshot!("fast_export_import", r.finish());
    assert_snapshot!("fast_import_into_empty", i.finish());
}
