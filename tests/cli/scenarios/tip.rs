//! rgit-only `tip` (`bingo`) and `zen`.
use crate::harness::Repo;
use insta::assert_snapshot;

#[test]
fn tips_and_zen() {
    // Neither needs a repository.
    let mut r = Repo::new();
    r.run("tip 1").run("bingo 7").run("tip --all").run("tip 0").run("tip 999").run("zen");
    assert_snapshot!(r.finish());
}
