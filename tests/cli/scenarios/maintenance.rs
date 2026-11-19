//! Object store maintenance: fsck, gc, prune, packs, bundles, archives.
use super::three_commits;
use insta::assert_snapshot;

#[test]
fn fsck_count_gc_prune() {
    let mut r = three_commits();
    r.run("fsck").run("count-objects").run("count-objects -v");
    r.write("dangling.txt", "nobody points here\n").run("hash-object -w dangling.txt").run("fsck").run("fsck --unreachable");
    r.run("prune -n").run("gc --dry-run").run("gc").run("count-objects -v").run("fsck");
    assert_snapshot!(r.finish());
}

#[test]
fn packs_roundtrip() {
    let mut r = three_commits();
    r.run("repack -a -d").run("count-objects -v").run("log --oneline").run("cat-file -p HEAD:a.txt").run("fsck");
    r.run("commit-graph write").run("commit-graph verify").run("multi-pack-index write").run("multi-pack-index verify");
    r.run("prune-packed -n");
    assert_snapshot!(r.finish());
}

#[test]
fn bundle_and_archive() {
    let mut r = three_commits();
    r.run("bundle create repo.bundle --all").run("bundle verify repo.bundle").run("bundle list-heads repo.bundle");
    r.run("archive --format=tar -o out.tar HEAD").run("archive --format=zip --prefix=p/ -o out.zip HEAD src");
    r.run("get-tar-commit-id");
    r.run("clone repo.bundle ../from-bundle");
    r.run_in("../from-bundle", "log --format=%s");
    assert_snapshot!(r.finish());
}
