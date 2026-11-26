//! Unit tests for `src/history/scrub/engine.rs`, compiled into that module via `#[path]`. The
//! rewrite itself is checked with real git in the `scrub` scenario.

use super::*;

#[test]
fn paths_are_normalized_and_bad_ones_refused() {
    let raw = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(normalize_paths(&raw(&["./config/secrets.env", "build\\", "build/", "a//b/"])).unwrap(), ["config/secrets.env", "build", "a/b"]);
    for bad in ["", "/etc/passwd", "../outside", "C:\\x", ".git/config", "."] {
        assert!(normalize_paths(&raw(&[bad])).is_err(), "{:?} should be refused", bad);
    }
}

#[test]
fn signatures_are_dropped_and_other_headers_kept() {
    let commit = b"tree aaaa\nparent bbbb\nauthor A <a@x> 1 +0000\ncommitter C <c@x> 2 +0000\nencoding ISO-8859-1\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n abc\n -----END PGP SIGNATURE-----\n\nmessage\n\nbody\n";
    let rewritten = String::from_utf8(rewrite_commit(commit, "tttt", &["pppp".to_string(), "qqqq".to_string()])).unwrap();
    assert_eq!(rewritten, "tree tttt\nparent pppp\nparent qqqq\nauthor A <a@x> 1 +0000\ncommitter C <c@x> 2 +0000\nencoding ISO-8859-1\n\nmessage\n\nbody\n");
}
