//! rgit-only `open` and `pr-summary`: web URLs for files, branches, commits and pull requests,
//! and a Markdown pull request description.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn open_prints_web_urls() {
    let mut r = three_commits();
    r.run("open -p");
    r.run("remote add origin git@github.com:sebi/rgit.git").run("update-ref refs/remotes/origin/main HEAD");
    r.run("open -p").run("open -p src/lib.txt:1").run("open -p a.txt:1-2").run("open -p src").run("open -p HEAD~1").run("open -p '#42'");
    r.run("switch -c feature/add-parser").run("open -p a.txt").run("open -p pr");
    r.run("update-ref refs/remotes/origin/feature/add-parser HEAD").run("open -p pr").run("open -p a.txt").run("open -p nope");
    // Other hosts.
    r.run("remote add lab https://gitlab.com/group/sub/proj.git").run("update-ref refs/remotes/lab/feature/add-parser HEAD");
    r.run("open -p --remote lab b.txt:3").run("open -p --remote lab '#7'");
    r.run("remote add bb ssh://git@bitbucket.org/team/repo.git").run("open -p --remote bb HEAD");
    r.run("remote add local ../elsewhere").run("open -p --remote local");
    assert_snapshot!(r.finish());
}

#[test]
fn pr_summary_markdown() {
    let mut r = three_commits();
    r.run("switch -c feature/add-parser");
    r.write("parser.rs", "fn parse() {}\n").run("add parser.rs").run("commit -m 'Add parser' -m 'Parses the input.'");
    r.run("pr-summary");
    r.write("a.txt", "one\nTWO\n").run("rm b.txt").run("commit -a -m 'Tidy a and b'");
    r.run("pr-summary").run("pr-summary main --title 'Parser, first cut'");
    // main moves on with a conflicting change.
    r.run("switch main").write("a.txt", "one\nzwei\n").run("commit -a -m 'German'").run("switch feature/add-parser");
    r.run("pr-summary main").run("pr-summary feature/add-parser");
    assert_snapshot!(r.finish());
}
