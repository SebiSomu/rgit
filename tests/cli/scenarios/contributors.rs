//! rgit-only `contributors`: who wrote the commits, with .mailmap and co-authors.
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn contributors_mailmap_coauthors_and_sorting() {
    let mut r = Repo::init();
    let commit = |r: &mut Repo, who: &str, email: &str, date: &str, file: &str, content: &str, msg: &str| {
        r.set_env("GIT_AUTHOR_NAME", who).set_env("GIT_AUTHOR_EMAIL", email).set_env("GIT_AUTHOR_DATE", date);
        r.write(file, content).run(&format!("add {}", file)).run(&format!("commit -m \"{}\"", msg));
    };
    commit(&mut r, "Ana", "ana@home.example", "2023-09-01T10:00:00 +0000", "a.txt", "a\nb\nc\n", "Start");
    commit(&mut r, "Bob", "bob@example.com", "2023-10-01T10:00:00 +0000", "b.txt", "b\n", "Bob's part");
    commit(&mut r, "ana", "ana@work.example", "2023-11-01T10:00:00 +0000", "a.txt", "a\nB\nc\n", "Fix b\n\nCo-authored-by: Cy <cy@example.com>");
    commit(&mut r, "Cy", "cy@example.com", "2023-11-02T10:00:00 +0000", "c.txt", "c\nc\n", "Cy's part");
    r.run("contributors");
    // .mailmap joins Ana's two identities.
    r.write(".mailmap", "Ana <ana@work.example> <ana@home.example>\nAna <ana@work.example> ana <ana@work.example>\n");
    r.run("contributors -e").run("contributors --sort first").run("contributors --sort lines").run("contributors --since 2023-10-15").run("contributors HEAD~1..");
    assert_snapshot!(r.finish());
}
