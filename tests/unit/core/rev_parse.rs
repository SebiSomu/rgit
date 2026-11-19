//! Unit tests for `src/core/rev_parse.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::*;
use std::sync::Mutex;

static TEST_LOCK: Mutex<()> = Mutex::new(());

/// resolve_rev should propagate the ambiguous-argument error when the name is garbage.
#[test]
fn test_resolve_rev_rejects_unknown_name() {
    let _guard = TEST_LOCK.lock().unwrap();
    let orig = std::env::current_dir().unwrap();
    let tmp = std::env::temp_dir().join(format!("rgit_rp_test_{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::env::set_current_dir(&tmp).unwrap();

    let err = resolve_rev("this_does_not_exist_at_all").unwrap_err();
    assert!(
        err.to_string().contains("unknown revision"),
        "unexpected error: {}",
        err
    );

    std::env::set_current_dir(orig).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_rev_parse_peel_and_prefix() {
    let _guard = TEST_LOCK.lock().unwrap();
    let orig = std::env::current_dir().unwrap();
    let tmp = std::env::temp_dir().join(format!("rgit_rp_test_peel_{}", std::process::id()));
    std::fs::create_dir_all(tmp.join(".git")).unwrap();
    std::env::set_current_dir(&tmp).unwrap();

    // Write a blob object
    let blob_data = b"hello test rev parse";
    let blob_hash = crate::core::object_store::write_object("blob", blob_data).unwrap();

    // Write a tree containing the blob
    let mut tree_content = Vec::new();
    let mode_and_name = format!("100644 hello.txt\0");
    tree_content.extend_from_slice(mode_and_name.as_bytes());
    let raw_hash = hex::decode(&blob_hash).unwrap();
    tree_content.extend_from_slice(&raw_hash);
    let tree_hash = crate::core::object_store::write_object("tree", &tree_content).unwrap();

    // Write a commit pointing to that tree
    let commit_data = format!(
        "tree {}\nauthor Test <test@example.com> 1234567890 +0000\ncommitter Test <test@example.com> 1234567890 +0000\n\ntest commit\n",
        tree_hash
    );
    let commit_hash = crate::core::object_store::write_object("commit", commit_data.as_bytes()).unwrap();

    // Test resolving full hash
    assert_eq!(resolve_rev(&commit_hash).unwrap(), commit_hash);

    // Test resolving prefix (short hash)
    let short_prefix = &commit_hash[..7];
    assert_eq!(resolve_rev(short_prefix).unwrap(), commit_hash);

    // Test peeling commit to tree
    let peel_spec = format!("{}^{{tree}}", commit_hash);
    assert_eq!(resolve_rev(&peel_spec).unwrap(), tree_hash);

    // Test work tree detection
    assert!(is_inside_work_tree());

    std::env::set_current_dir(orig).unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
}
