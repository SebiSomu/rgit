//! rgit-only `stats`: activity, weekdays, authors, languages and the biggest commits.
use crate::harness::Repo;
use insta::assert_snapshot;

/// Commits on chosen dates (author time) by chosen authors.
fn history() -> Repo {
    let mut r = Repo::init();
    let mut commit = |r: &mut Repo, who: &str, date: &str, file: &str, content: &str, msg: &str| {
        r.set_env("GIT_AUTHOR_NAME", who).set_env("GIT_AUTHOR_DATE", date);
        r.write(file, content).run(&format!("add {}", file)).run(&format!("commit -m '{}'", msg));
    };
    commit(&mut r, "Ana", "2023-09-04T10:00:00 +0300", "src/main.rs", "fn main() {\n    run();\n}\n", "Start");
    commit(&mut r, "Ana", "2023-09-05T23:30:00 -0500", "README.md", "# Demo\n\nIt runs.\n", "Readme");
    commit(&mut r, "Bob", "2023-10-02T09:00:00 +0000", "src/lib.rs", "pub fn run() {}\npub fn a() {}\npub fn b() {}\npub fn c() {}\n", "Library");
    commit(&mut r, "Ana", "2023-10-03T09:00:00 +0000", "src/main.rs", "fn main() {\n    lib::run();\n}\n", "Use lib");
    commit(&mut r, "Cy", "2023-11-11T12:00:00 +0000", "web/app.ts", "export const x = 1;\n", "Web app");
    commit(&mut r, "Bob", "2023-11-13T08:00:00 +0000", "src/lib.rs", "pub fn run() {}\n", "Trim lib");
    r
}

#[test]
fn stats_overview() {
    let mut r = history();
    r.run("stats").run("stats --weeks 4 src").run("stats --since 2023-11-01").run("stats --since 2030-01-01");
    assert_snapshot!(r.finish());
}
