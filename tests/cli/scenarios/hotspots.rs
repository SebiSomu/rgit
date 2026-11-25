//! rgit-only `hotspots`: files that change most, have most authors, and change together.
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn hotspots_churn_authors_and_coupling() {
    let mut r = Repo::init();
    r.set_env("GIT_AUTHOR_NAME", "Ana");
    r.write("cli.rs", "a\n").write("parser.rs", "p\n").write("README.md", "r\n").run("add .").run("commit -m start");
    r.write("cli.rs", "a\nb\n").write("parser.rs", "p\nq\n").run("commit -a -m 'cli + parser'");
    r.set_env("GIT_AUTHOR_NAME", "Bob");
    r.write("cli.rs", "a\nb\nc\n").write("parser.rs", "P\nq\n").run("commit -a -m 'cli + parser again'");
    r.write("cli.rs", "A\nb\nc\n").run("commit -a -m 'cli only'");
    r.set_env("GIT_AUTHOR_NAME", "Cy");
    r.write("README.md", "r\nmore\n").write("gone.txt", "g\n").run("add .").run("commit -m docs");
    r.run("rm gone.txt").run("commit -m 'drop gone'");
    // A bulk commit (more than 30 files) counts as a change but not for coupling.
    for i in 0..31 {
        r.write(&format!("gen/f{:02}.txt", i), "x\n");
    }
    r.write("cli.rs", "A\nb\nc\nd\n").run("add .").run("commit -m 'generate files'");
    r.run("hotspots").run("hotspots -n 1").run("hotspots README.md").run("hotspots --since '1 day ago'").run("hotspots --since nonsense");
    assert_snapshot!(r.finish());
}
