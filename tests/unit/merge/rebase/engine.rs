//! Unit tests for `src/merge/rebase/engine.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::*;

#[test]
fn test_split_word() {
    assert_eq!(split_word("pick 1234567 initial commit"), ("pick", "1234567 initial commit"));
    assert_eq!(split_word("  drop   abcd  "), ("drop", "abcd  "));
    assert_eq!(split_word("word"), ("word", ""));
}

#[test]
fn test_parse_edited_todo_meld() {
    let candidates = vec![
        TodoItem {
            action: Action::Pick,
            hash: "1111111111111111111111111111111111111111".to_string(),
            subject: "first".to_string(),
        },
        TodoItem {
            action: Action::Pick,
            hash: "2222222222222222222222222222222222222222".to_string(),
            subject: "second".to_string(),
        },
    ];

    // Without allow_first_meld, first item cannot be squash/fixup:
    let text_squash_first = "squash 1111111 first\npick 2222222 second\n";
    assert!(parse_edited_todo_with_meld(text_squash_first, &candidates, false).is_err());

    // With allow_first_meld = true, first item can be squash/fixup (into HEAD):
    let parsed = parse_edited_todo_with_meld(text_squash_first, &candidates, true).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].action, Action::Squash);
    assert_eq!(parsed[1].action, Action::Pick);

    // Standard pick followed by squash works in both:
    let text_normal = "pick 1111111 first\nsquash 2222222 second\n";
    assert!(parse_edited_todo(text_normal, &candidates).is_ok());
}
