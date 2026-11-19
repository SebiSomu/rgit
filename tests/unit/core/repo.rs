//! Unit tests for `src/core/repo.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::is_shared;

#[test]
fn routes_shared_and_per_worktree_paths() {
    assert!(is_shared("objects/ab/cdef"));
    assert!(is_shared("refs/heads/main"));
    assert!(is_shared("logs/refs/heads/main"));
    assert!(is_shared("config"));
    assert!(is_shared("STASH_LIST"));
    assert!(is_shared("logs"));
    assert!(!is_shared("HEAD"));
    assert!(!is_shared("index"));
    assert!(!is_shared("logs/HEAD"));
    assert!(!is_shared("MERGE_HEAD"));
    assert!(!is_shared("rebase-apply/next"));
    assert!(!is_shared("refs/bisect/bad"));
    assert!(!is_shared("logs/refs/bisect/bad"));
    assert!(!is_shared("refs/worktree"));
}
