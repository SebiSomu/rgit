use super::{diverged, three_commits};
use insta::assert_snapshot;

#[test]
fn log_formats() {
    let mut r = three_commits();
    r.run("log").run("log --oneline -2").run("log --stat").run("log -p -1").run("log --name-status");
    r.run("log --format='%H%n%h|%T|%P|%an|%ae|%ad|%cn|%s|%b' --date=iso").run("log --pretty=fuller -1");
    r.run("log --reverse --format=%s").run("log --format=%s -- src").run("log --date=relative -1 --format=%ad");
    r.run("log --decorate --oneline").run("log --raw -1");
    assert_snapshot!(r.finish());
}

#[test]
fn log_graph_and_all() {
    let mut r = diverged();
    r.run("merge feature -m merged").run("log --graph --oneline --all").run("log --graph --format='%s'");
    r.run("log --first-parent --format=%s").run("log --merges --format=%s").run("log --no-merges --format=%s");
    assert_snapshot!(r.finish());
}

#[test]
fn log_follow_rename() {
    let mut r = three_commits();
    r.run("mv a.txt renamed.txt").run("commit -m rename");
    r.write("renamed.txt", "one\ntwo\nthree\n").run("commit -a -m 'edit after rename'");
    r.run("log --follow --format=%s -- renamed.txt").run("log --format=%s -- renamed.txt").run("log --follow --stat -- renamed.txt");
    assert_snapshot!(r.finish());
}

#[test]
fn show_objects() {
    let mut r = three_commits();
    r.run("show").run("show --stat HEAD~1").run("show HEAD:a.txt").run("show HEAD^{tree}").run("show --format=%s --no-patch");
    assert_snapshot!(r.finish());
}

#[test]
fn diff_variants() {
    let mut r = three_commits();
    r.write("a.txt", "one\nTWO\nthree\n").write("b.txt", "bee\nsee\n").run("add b.txt");
    r.run("diff").run("diff --cached").run("diff HEAD").run("diff --stat").run("diff --name-only HEAD~2");
    r.run("diff --name-status HEAD~2 HEAD").run("diff HEAD~2..HEAD -- src").run("diff --numstat HEAD~1").run("diff -U1 HEAD~2");
    r.run("diff --raw HEAD~1 HEAD").run("diff --shortstat HEAD~2");
    assert_snapshot!(r.finish());
}

#[test]
fn diff_renames() {
    let mut r = three_commits();
    r.write("big.txt", "line a\nline b\nline c\nline d\nline e\nline f\n").run("add big.txt").run("commit -m big");
    r.run("mv big.txt moved.txt");
    r.write("moved.txt", "line a\nline b\nline c\nline d\nline e\nline F\n").run("add moved.txt");
    r.run("diff --cached --stat").run("diff --cached -M --name-status").run("diff --cached --no-renames --name-status");
    assert_snapshot!(r.finish());
}

#[test]
fn rev_parse_and_rev_list() {
    let mut r = diverged();
    r.run("rev-parse HEAD").run("rev-parse --short HEAD").run("rev-parse --abbrev-ref HEAD").run("rev-parse HEAD~1 main^ feature@{0}");
    r.run("rev-parse --show-toplevel").run("rev-parse --git-dir").run("rev-parse --is-inside-work-tree").run("rev-parse --verify nope");
    r.run("rev-list --count HEAD").run("rev-list main..feature").run("rev-list --left-right main...feature").run("rev-list --all --topo-order");
    r.run("rev-list --max-count=1 feature").run("rev-list --parents feature");
    assert_snapshot!(r.finish());
}

#[test]
fn blame_and_annotate() {
    let mut r = three_commits();
    r.write("a.txt", "one\ntwo\nthree\n").run("commit -a -m 'third line'");
    r.run("blame a.txt").run("blame -L 2,3 a.txt").run("blame -s a.txt").run("annotate a.txt");
    r.write("a.txt", "one\nchanged\nthree\n").run("blame a.txt");
    assert_snapshot!(r.finish());
}

#[test]
fn grep_and_shortlog() {
    let mut r = three_commits();
    r.run("grep one").run("grep -n lib").run("grep -i ONE").run("grep -c e").run("grep -l v").run("grep --cached bee").run("grep lib HEAD~2");
    r.run("grep nomatch");
    r.run("shortlog").run("shortlog -s -n").run("shortlog -e");
    assert_snapshot!(r.finish());
}

#[test]
fn refs_listing() {
    let mut r = diverged();
    r.run("tag v1").run("tag -a v2 -m annotated feature");
    r.run("show-ref").run("show-ref --heads").run("show-ref --tags -d").run("show-ref --verify refs/heads/main").run("show-ref --verify refs/heads/missing");
    r.run("for-each-ref").run("for-each-ref --format='%(refname:short) %(objecttype) %(subject)' --sort=-refname");
    r.run("for-each-ref --count=1 refs/heads").run("for-each-ref --format='%(if)%(upstream)%(then)up%(else)none%(end) %(refname)'");
    r.run("name-rev HEAD feature~1").run("show-branch main feature").run("cherry main feature").run("cherry -v main feature");
    assert_snapshot!(r.finish());
}

#[test]
fn reflog_entries() {
    let mut r = diverged();
    r.run("reflog").run("reflog show feature").run("reflog exists main").run("reflog exists nothing");
    r.run("reset --hard HEAD~1").run("reflog -n 2").run("rev-parse HEAD@{1}");
    assert_snapshot!(r.finish());
}

#[test]
fn whatchanged_last_modified_range_diff() {
    let mut r = diverged();
    r.run("whatchanged -1").run("last-modified").run("last-modified -r");
    r.run("branch feature-v2 feature").run("switch feature-v2");
    r.write("feature.txt", "on feature\nmore\nv2\n").run("commit -a --amend -m 'more feature work v2'");
    r.run("range-diff main..feature main..feature-v2");
    assert_snapshot!(r.finish());
}
