//! Unit tests for `src/helpers/add_patch.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::{apply_hunks, parse_hunk_header, split_hunk};

fn hunk(header: &str, body: &[&str]) -> crate::objects::InteractiveHunk {
    let mut h = parse_hunk_header(format!("{}\n", header).as_bytes()).unwrap();
    h.lines = body.iter().map(|l| format!("{}\n", l).into_bytes()).collect();
    h
}

#[test]
fn split_shares_context_like_git() {
    let h = hunk("@@ -1,9 +1,9 @@", &[" 1", "-2", "+B", " 3", " 4", " 5", "-6", "+F", " 7"]);
    let parts = split_hunk(&h);
    assert_eq!(parts.len(), 2);
    assert_eq!((parts[0].old_offset, parts[0].old_count, parts[0].new_offset, parts[0].new_count), (1, 5, 1, 5));
    assert_eq!((parts[1].old_offset, parts[1].old_count, parts[1].new_offset, parts[1].new_count), (3, 5, 3, 5));
    assert_eq!(parts[1].lines[0], b" 3\n");
}

#[test]
fn applies_one_half_of_a_split_and_both_halves() {
    let base = b"1\n2\n3\n4\n5\n6\n7\n";
    let h = hunk("@@ -1,7 +1,7 @@", &[" 1", "-2", "+B", " 3", " 4", " 5", "-6", "+F", " 7"]);
    let parts = split_hunk(&h);
    assert_eq!(apply_hunks(base, &[&parts[1]], false).unwrap(), b"1\n2\n3\n4\n5\nF\n7\n");
    assert_eq!(apply_hunks(base, &[&parts[0], &parts[1]], false).unwrap(), b"1\nB\n3\n4\n5\nF\n7\n");
    // Reverse: take the change back out of the new side.
    assert_eq!(apply_hunks(b"1\nB\n3\n4\n5\nF\n7\n", &[&parts[0]], true).unwrap(), b"1\n2\n3\n4\n5\nF\n7\n");
}
