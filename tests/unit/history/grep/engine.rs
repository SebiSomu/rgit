//! Unit tests for `src/history/grep/engine.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::*;

#[test]
fn test_build_grep_matcher() {
    let regex = build_grep_matcher("hello.*world", false, false).unwrap();
    assert!(regex.is_match("hello beautiful world"));
    assert!(!regex.is_match("HELLO BEAUTIFUL WORLD"));

    let case_insensitive = build_grep_matcher("hello", false, true).unwrap();
    assert!(case_insensitive.is_match("HELLO"));

    let fixed = build_grep_matcher("a+b", true, false).unwrap();
    assert!(fixed.is_match("a+b"));
    assert!(!fixed.is_match("aaaaab"));
}

#[test]
fn test_matches_pathspec() {
    assert!(matches_pathspec("src/main.rs", &[]));
    assert!(matches_pathspec("src/main.rs", &["src/".to_string()]));
    assert!(matches_pathspec("src/main.rs", &["*.rs".to_string()]));
    assert!(!matches_pathspec("src/main.rs", &["tests/".to_string()]));
}

#[test]
fn test_is_binary_content() {
    assert!(!is_binary_content(b"hello world\n"));
    assert!(is_binary_content(b"hello\0world\n"));
}

#[test]
fn test_search_content() {
    let options = GrepOptions {
        ignore_case: false,
        invert_match: false,
        line_number: true,
        count: false,
        files_with_matches: false,
        files_without_match: false,
        fixed_strings: false,
        cached: false,
        color: false,
    };
    let matcher = build_grep_matcher("apple", false, false).unwrap();
    let content = b"one apple\ntwo bananas\nthree apples\n";
    let res = search_content("test.txt", content, &matcher, &options);

    assert!(!res.is_binary);
    assert_eq!(res.matches.len(), 2);
    assert_eq!(res.matches[0], (1, "one apple".to_string()));
    assert_eq!(res.matches[1], (3, "three apples".to_string()));
}
