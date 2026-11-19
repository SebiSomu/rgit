//! Unit tests for `src/workspace/worktree/engine.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::*;
use std::path::PathBuf;

fn info(path: &str, branch: Option<&str>, locked: Option<&str>, prunable: Option<&str>) -> WorktreeInfo {
    WorktreeInfo {
        path: PathBuf::from(path),
        admin_dir: PathBuf::from("/repo/.git"),
        id: if path == "/repo" { None } else { Some("x".to_string()) },
        head: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
        branch: branch.map(str::to_string),
        locked: locked.map(str::to_string),
        prunable: prunable.map(str::to_string),
        is_current: false,
    }
}

#[test]
fn list_pads_paths_and_flags_states() {
    let wts = vec![
        info("/repo", Some("refs/heads/main"), None, None),
        info("/wt/feature", None, Some(""), None),
        info("/wt/gone", Some("refs/heads/gone"), None, Some("gitdir file points to non-existent location")),
    ];
    assert_eq!(
        format_worktree_list(&wts, false),
        "/repo        0123456 [main]\n\
         /wt/feature  0123456 (detached HEAD) locked\n\
         /wt/gone     0123456 [gone] prunable\n"
    );
    assert_eq!(
        format_worktree_list(&wts, true),
        "/repo        0123456 [main]\n\
         /wt/feature  0123456 (detached HEAD) locked\n\
         /wt/gone     0123456 [gone]\n\
         \tprunable: gitdir file points to non-existent location\n"
    );
}

#[test]
fn porcelain_has_one_stanza_per_worktree() {
    let wts = vec![
        info("/repo", Some("refs/heads/main"), None, None),
        info("/wt/feature", None, Some("usb disk"), None),
    ];
    assert_eq!(
        format_worktree_porcelain(&wts),
        "worktree /repo\nHEAD 0123456789abcdef0123456789abcdef01234567\nbranch refs/heads/main\n\n\
         worktree /wt/feature\nHEAD 0123456789abcdef0123456789abcdef01234567\ndetached\nlocked usb disk\n\n"
    );
}
