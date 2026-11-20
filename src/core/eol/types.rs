//! Types for the line-ending conversion in `src/core/eol.rs`.

/// The `text` attribute of a path.
// Used by: src/core/eol.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAttr {
    Unspecified,
    Set,
    Unset,
    Auto,
}

/// The `eol` attribute of a path.
// Used by: src/core/eol.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EolAttr {
    Unspecified,
    Lf,
    Crlf,
}

/// What git calls the crlf action: whether a path is converted, and how.
// Used by: src/core/eol.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// No conversion.
    Binary,
    /// Always text: CRLF -> LF on the way in, nothing on the way out.
    TextInput,
    /// Always text: CRLF -> LF on the way in, LF -> CRLF on the way out.
    TextCrlf,
    /// Text if it looks like text: CRLF -> LF on the way in, nothing on the way out.
    AutoInput,
    /// Text if it looks like text: CRLF -> LF on the way in, LF -> CRLF on the way out.
    AutoCrlf,
}

/// `core.autocrlf`.
// Used by: src/core/eol.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoCrlf {
    False,
    True,
    Input,
}

/// The settings that decide the action for every path.
// Used by: src/core/eol.rs
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub autocrlf: AutoCrlf,
    /// `core.eol` resolved to the checkout line ending (`native` is CRLF on Windows).
    pub eol_crlf: bool,
}

/// One line of a `.gitattributes` file, reduced to the attributes that affect line endings.
// Used by: src/core/eol.rs
#[derive(Clone, Debug)]
pub(super) struct AttrLine {
    pub(super) pattern: String,
    pub(super) text: Option<TextAttr>,
    pub(super) eol: Option<EolAttr>,
}
