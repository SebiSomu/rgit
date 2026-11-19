use super::three_commits;
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn init_add_commit() {
    let mut r = Repo::init();
    r.write("a.txt", "hello\n").write("dir/b.txt", "world\n");
    r.run("status").run("add a.txt dir").run("status").run("commit -m first");
    r.write("a.txt", "hello again\n");
    r.run("status").run("commit -a -m second").run("log --oneline");
    assert_snapshot!(r.finish());
}

#[test]
fn init_twice_and_bare_dir() {
    let mut r = Repo::new();
    r.run("status").run("init").run("init");
    assert_snapshot!(r.finish());
}

#[test]
fn status_short_and_porcelain() {
    let mut r = three_commits();
    r.write("a.txt", "changed\n").write("new.txt", "new\n").remove("b.txt");
    r.write("src/lib.txt", "staged\n").run("add src/lib.txt");
    r.write("src/lib.txt", "staged then modified\n");
    r.run("status").run("status --short").run("status -s").run("status --porcelain");
    assert_snapshot!(r.finish());
}

#[test]
fn status_from_subdirectory() {
    let mut r = three_commits();
    r.write("src/new.txt", "x\n").write("top.txt", "y\n");
    r.run_in("src", "status").run_in("src", "add new.txt").run_in("src", "status --short");
    assert_snapshot!(r.finish());
}

#[test]
fn commit_variants() {
    let mut r = three_commits();
    r.write("c.txt", "c\n").run("add c.txt");
    r.run("commit -m 'subject' -m 'body line' -s");
    r.run("commit --amend -m 'amended subject'");
    r.write("msg.txt", "from file\n\nfile body\n");
    r.write("a.txt", "file commit\n").run("add a.txt");
    r.run("commit -F msg.txt");
    r.write("a.txt", "authored\n").run("add a.txt");
    r.run("commit -m authored --author 'Other Person <other@example.com>'");
    r.run("commit -m 'nothing to commit'");
    r.run("commit --allow-empty -m empty");
    r.run("log --format='%h %an <%ae> | %cn | %s'");
    assert_snapshot!(r.finish());
}

#[test]
fn add_deletions_and_all() {
    let mut r = three_commits();
    r.remove("b.txt").run("add b.txt").run("status --short");
    r.remove("a.txt").write("n.txt", "n\n").run("add -A").run("status --short");
    r.run("add .").run("add nothing-here");
    assert_snapshot!(r.finish());
}

#[test]
fn rm_and_mv() {
    let mut r = three_commits();
    r.run("rm b.txt").run("status --short");
    r.run("rm --cached a.txt").run("status --short");
    r.run("add a.txt").run("mv src/lib.txt src/renamed.txt").run("status --short");
    r.run("mv a.txt moved").run("status");
    r.run("rm nonexistent.txt");
    r.write("src/renamed.txt", "dirty\n").run("rm src/renamed.txt");
    r.run("commit -m 'moves and removes'");
    assert_snapshot!(r.finish());
}

#[test]
fn restore_and_reset() {
    let mut r = three_commits();
    r.write("a.txt", "dirty\n").run("restore a.txt").run("status --short");
    r.write("a.txt", "staged\n").run("add a.txt").run("restore --staged a.txt").run("status --short");
    r.run("restore a.txt");
    r.run("restore --source HEAD~2 src/lib.txt").run("status --short").run("restore src/lib.txt");
    r.run("reset --soft HEAD~1").run("status --short").checkpoint("after soft");
    r.run("reset HEAD~1").run("status --short").checkpoint("after mixed");
    r.run("reset --hard HEAD").run("status --short");
    r.run("reset --hard HEAD@{1}");
    r.run("log --oneline");
    assert_snapshot!(r.finish());
}

#[test]
fn clean_modes() {
    let mut r = three_commits();
    r.write(".gitignore", "*.log\n").run("add .gitignore").run("commit -m ignore");
    r.write("junk.txt", "j\n").write("debug.log", "l\n").write("tmp/x.txt", "x\n");
    r.run("clean").run("clean -n").run("clean -n -d").run("clean -n -x").run("clean -n -X");
    r.run("clean -f").checkpoint("after clean -f");
    r.run("clean -f -d").run("clean -f -x");
    assert_snapshot!(r.finish());
}

#[test]
fn gitignore_rules() {
    let mut r = Repo::init();
    r.write(".gitignore", "*.o\n!keep.o\nbuild/\n/root-only.txt\ndocs/**/*.tmp\n");
    r.write("a.o", "").write("keep.o", "").write("build/out", "").write("root-only.txt", "");
    r.write("sub/root-only.txt", "").write("docs/x/y/z.tmp", "").write("docs/ok.md", "");
    r.run("status --short").run("add .").run("status --short");
    r.run("check-ignore -v a.o keep.o build/out root-only.txt sub/root-only.txt docs/x/y/z.tmp docs/ok.md");
    r.run("ls-files -o -i --exclude-standard");
    assert_snapshot!(r.finish());
}

#[test]
fn config_get_set() {
    let mut r = Repo::init();
    r.run("config user.name 'Config Name'").run("config user.email cfg@example.com");
    r.run("config user.name").run("config --list");
    r.run("config core.fancy.value 'with spaces'").run("config core.fancy.value");
    r.run("config --unset core.fancy.value").run("config core.fancy.value");
    r.run("config --global alias.st status").run("config --global --list");
    r.unset_env("GIT_AUTHOR_NAME").unset_env("GIT_AUTHOR_EMAIL");
    r.write("f", "f\n").run("add f").run("commit -m 'uses config identity'");
    r.run("log -1 --format='%an <%ae>'");
    r.run("config not-a-valid-key value");
    assert_snapshot!(r.finish());
}
