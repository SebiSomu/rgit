//! rgit-only `changelog`: Markdown from conventional commits, per version.
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn changelog_sections_tags_and_prepend() {
    let mut r = Repo::init();
    let mut commit = |r: &mut Repo, file: &str, who: &str, msg: &str| {
        r.set_env("GIT_AUTHOR_NAME", who);
        r.write(file, &format!("{}\n", msg.len())).run(&format!("add {}", file)).run(&format!("commit -m \"{}\"", msg));
    };
    commit(&mut r, "a.txt", "Ana", "feat: first release");
    commit(&mut r, "a.txt", "Ana", "fix(parser): handle empty input");
    r.run("tag v0.1.0");
    commit(&mut r, "b.txt", "Bob", "feat(cli): add --json output");
    commit(&mut r, "b.txt", "Bob", "docs: explain --json");
    commit(&mut r, "c.txt", "Ana", "feat(api)!: rename run() to start()");
    commit(&mut r, "c.txt", "Cy", "ci: cache cargo builds");
    commit(&mut r, "d.txt", "Cy", "Tweak the README wording");
    r.run("changelog").run("changelog --version v0.2.0").run("changelog v0.1.0..HEAD~3").run("changelog --all");
    r.run("tag v0.2.0").run("changelog").run("changelog --all --prepend CHANGELOG.md").run("changelog --all --prepend CHANGELOG.md");
    assert_snapshot!(r.finish());
}
