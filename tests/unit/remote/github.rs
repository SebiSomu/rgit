//! Unit tests for `src/remote/github.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::*;

#[test]
fn test_format_unix_timestamp_iso8601() {
    assert_eq!(format_unix_timestamp_iso8601(0), "1970-01-01T00:00:00Z");
    assert_eq!(format_unix_timestamp_iso8601(1712838123), "2024-04-11T12:22:03Z");
    assert_eq!(format_unix_timestamp_iso8601(1775898000), "2026-04-11T09:00:00Z");
}

#[test]
fn test_parse_commit_identity() {
    let line = "author Linus Torvalds <torvalds@linux-foundation.org> 1712838123 +0000";
    let ident = parse_commit_identity(line).expect("Should parse author line");
    assert_eq!(ident.name, "Linus Torvalds");
    assert_eq!(ident.email, "torvalds@linux-foundation.org");
    assert_eq!(ident.date, "2024-04-11T12:22:03Z");

    let committer_line = "committer Octocat <octo@github.com> 0 +0200";
    let committer = parse_commit_identity(committer_line).expect("Should parse committer");
    assert_eq!(committer.name, "Octocat");
    assert_eq!(committer.email, "octo@github.com");
    assert_eq!(committer.date, "1970-01-01T00:00:00Z");
}

#[test]
fn test_parse_github_remote() {
    assert_eq!(
        parse_github_remote("https://github.com/SebiSomu/RGit.git"),
        Some(("SebiSomu".to_string(), "RGit".to_string()))
    );
    assert_eq!(
        parse_github_remote("https://github.com/owner/my-repo"),
        Some(("owner".to_string(), "my-repo".to_string()))
    );
    assert_eq!(
        parse_github_remote("git@github.com:rust-lang/cargo.git"),
        Some(("rust-lang".to_string(), "cargo".to_string()))
    );
    assert_eq!(
        parse_github_remote("git@github.com:owner/repo"),
        Some(("owner".to_string(), "repo".to_string()))
    );
    assert_eq!(
        parse_github_remote("github.com/torvalds/linux"),
        Some(("torvalds".to_string(), "linux".to_string()))
    );
    assert_eq!(
        parse_github_remote("nonexistent_owner/nonexistent_repo"),
        Some(("nonexistent_owner".to_string(), "nonexistent_repo".to_string()))
    );
    assert_eq!(parse_github_remote("./local/dir"), None);
    assert_eq!(parse_github_remote("local_repo"), None);
}

#[test]
fn test_iso8601_to_git_timestamp() {
    assert_eq!(iso8601_to_git_timestamp("1970-01-01T00:00:00Z"), "0 +0000");
    // 2024-01-01 00:00:00 UTC = 1704067200
    assert_eq!(iso8601_to_git_timestamp("2024-01-01T00:00:00Z"), "1704067200 +0000");
}

#[test]
fn test_build_tree_content_matches_git() {
    // GitHub reports the directory as `040000`; git (and `git mktree`) stores `40000`.
    // `dir` must sort after `dir.txt`, because git compares directories as `dir/`.
    let items: Vec<serde_json::Value> = serde_json::from_str(r#"[
        {"path": "dir", "mode": "040000", "type": "tree", "sha": "488958086aadb031695daea66ee5dec69cb80f15"},
        {"path": "dir.txt", "mode": "100644", "type": "blob", "sha": "45b983be36b73c0788dc9cbcb76cbb80fc7bb057"}
    ]"#).unwrap();
    let content = build_tree_content(&items).unwrap();
    let hash = hex::encode(crate::core::object_store::hash_content("tree", &content));
    assert_eq!(hash, "fb372e232541e395468645882a435bae01be6db3");
}

#[test]
fn test_ensure_same_object() {
    assert!(ensure_same_object("tree", "abc", "abc").is_ok());
    assert!(ensure_same_object("commit", "abc", "def").is_err());
}
