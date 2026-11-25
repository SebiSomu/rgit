//! Types of `rgit changelog`.

/// One commit as a changelog entry: its conventional-commit type (`feat`, `fix`, ...; None
/// when the subject doesn't follow the convention), scope, description, whether it is a
/// breaking change (`!` or a `BREAKING CHANGE:` footer, with that footer's text), short hash
/// and author.
// Used by: src/history/changelog/types.rs, src/history/changelog/engine.rs, src/history/changelog.rs
// Used by command/helper: `parse_entry`, `render_section` and the `changelog` command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Option<String>,
    pub scope: Option<String>,
    pub text: String,
    pub breaking: Option<String>,
    pub hash: String,
    pub author: String,
}
