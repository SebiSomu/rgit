//! Unit tests for `src/helpers/shortlog.rs`, kept outside `src/` and compiled into that module via
//! `#[path]` (so they can reach its private items through `super::`).

use super::*;

#[test]
fn test_parse_author_key_name_only() {
    let raw = "Alice Smith <alice@example.com> 1234567890 +0000";
    assert_eq!(parse_author_key(raw, false), "Alice Smith");
}

#[test]
fn test_parse_author_key_with_email() {
    let raw = "Alice Smith <alice@example.com> 1234567890 +0000";
    assert_eq!(parse_author_key(raw, true), "Alice Smith <alice@example.com>");
}

#[test]
fn test_parse_author_key_no_email_field() {
    let raw = "Bob 1234567890 +0000";
    assert_eq!(parse_author_key(raw, false), "Bob");
}

#[test]
fn test_strip_author_timestamp() {
    let s = "Alice <a@b.com> 1700000000 +0300";
    assert_eq!(strip_author_timestamp(s), "Alice <a@b.com>");
}

#[test]
fn test_strip_author_timestamp_no_timestamp() {
    // If the string doesn't end with a well-formed timestamp, return it unchanged
    let s = "No Timestamp Here";
    assert_eq!(strip_author_timestamp(s), "No Timestamp Here");
}
