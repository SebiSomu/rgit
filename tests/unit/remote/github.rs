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
    // The commit's own zone is kept, so GitHub rebuilds the same hash.
    assert_eq!(committer.date, "1970-01-01T02:00:00+02:00");
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

/// Builds a raw tree object from `(mode, name, hex hash)` entries.
fn raw_tree(entries: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (mode, name, hash) in entries {
        out.extend_from_slice(format!("{} {}\0", mode, name).as_bytes());
        out.extend_from_slice(&hex::decode(hash).unwrap());
    }
    out
}

#[test]
fn test_collect_known_tree_objects_marks_the_remote_tree_as_uploaded() {
    let (root, sub, a, b, c, gitlink) = ("1".repeat(40), "2".repeat(40), "a".repeat(40), "b".repeat(40), "c".repeat(40), "d".repeat(40));
    let mut objects: HashMap<String, (String, Vec<u8>)> = HashMap::new();
    objects.insert(root.clone(), ("tree".into(), raw_tree(&[("100644", "a.txt", &a), ("40000", "src", &sub), ("160000", "vendor", &gitlink)])));
    objects.insert(sub.clone(), ("tree".into(), raw_tree(&[("100755", "run.sh", &b), ("120000", "link", &c)])));
    let read = |hash: &str| objects.get(hash).cloned().ok_or_else(|| anyhow::anyhow!("missing {}", hash));

    let mut trees = HashMap::new();
    let mut blobs = HashSet::new();
    collect_known_tree_objects(&root, &read, &mut trees, &mut blobs).unwrap();

    // Trees map to themselves (same hash on GitHub); the submodule commit is not a blob.
    assert_eq!(trees.get(&root), Some(&root));
    assert_eq!(trees.get(&sub), Some(&sub));
    assert_eq!(blobs, HashSet::from([a, b, c]));
}

#[test]
fn test_collect_known_tree_objects_keeps_what_it_found_when_an_object_is_missing() {
    let (root, missing, a) = ("1".repeat(40), "2".repeat(40), "a".repeat(40));
    let mut objects: HashMap<String, (String, Vec<u8>)> = HashMap::new();
    objects.insert(root.clone(), ("tree".into(), raw_tree(&[("100644", "a.txt", &a), ("40000", "gone", &missing)])));
    let read = |hash: &str| objects.get(hash).cloned().ok_or_else(|| anyhow::anyhow!("missing {}", hash));

    let mut trees = HashMap::new();
    let mut blobs = HashSet::new();
    assert!(collect_known_tree_objects(&root, &read, &mut trees, &mut blobs).is_err());
    assert!(trees.contains_key(&root) && !trees.contains_key(&missing));
    assert!(blobs.contains(&a));
}

#[test]
fn test_parse_commit_identity_keeps_the_time_zone() {
    let ident = parse_commit_identity("author Sebi <s@x.ro> 1795687650 +0200").unwrap();
    // 1795687650 is 2026-11-26T10:07:30Z, which is 12:07:30 at +02:00.
    assert_eq!(ident.date, "2026-11-26T12:07:30+02:00");
    let ident = parse_commit_identity("committer A <a@b> 1795687650 -0530").unwrap();
    assert_eq!(ident.date, "2026-11-26T04:37:30-05:30");
    // UTC keeps the `Z` form; a missing or broken zone counts as UTC.
    assert_eq!(parse_commit_identity("author A <a@b> 1795687650 +0000").unwrap().date, "2026-11-26T10:07:30Z");
    assert_eq!(parse_commit_identity("author A <a@b> 1795687650").unwrap().date, "2026-11-26T10:07:30Z");
    assert_eq!(parse_commit_identity("author A <a@b> 1795687650 bogus").unwrap().date, "2026-11-26T10:07:30Z");
}

#[test]
fn test_ensure_pushed_commit_matches() {
    let local = "357620c0000000000000000000000000000000000";
    assert!(ensure_pushed_commit_matches(local, local, "main").is_ok());
    let err = ensure_pushed_commit_matches(local, "ebda604000000000000000000000000000000000", "dev").unwrap_err().to_string();
    assert!(err.contains("357620c as ebda604"));
    assert!(err.contains("Nothing was updated: 'dev'"));
}

#[test]
fn test_raw_commit_message_keeps_every_byte() {
    let commit = "tree t\nparent p\nauthor A <a@b> 1 +0000\ncommitter A <a@b> 1 +0000\n\nSubject\n\nBody line\n";
    assert_eq!(raw_commit_message(commit), "Subject\n\nBody line\n");
    assert_eq!(raw_commit_message("tree t\n\nno newline"), "no newline");
    assert_eq!(raw_commit_message("tree t\n\n"), "");
    assert_eq!(raw_commit_message("tree t\n"), "");
}
