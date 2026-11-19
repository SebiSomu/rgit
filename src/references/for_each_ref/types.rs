/// How `for-each-ref` quotes each interpolated value (`-s`/`--shell`, `-p`/`--perl`,
/// `--python`, `--tcl`), so the output can be `eval`ed by that language.
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_for_each_ref_args` and `quote_ref_value`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RefQuoteStyle {
    #[default]
    None,
    Shell,
    Perl,
    Python,
    Tcl,
}

/// Parsed arguments for the `for-each-ref` command. Parsed by hand like git's option parser
/// (abbreviated long options, `--no-*` negations, `--merged`/`--contains` defaulting to HEAD
/// when last), so errors can exit 129/128 as git does. Revisions are already resolved.
// Used by: src/references/for_each_ref/engine.rs, src/workspace/branch/engine.rs
// Used by command/helper: `parse_for_each_ref_args` and the `for_each_ref` command
#[derive(Debug, Clone, Default)]
pub struct ForEachRefOptions {
    /// Quoting style; `quote_bits` counts how many were asked for (more than one is an error).
    pub quote: RefQuoteStyle,
    pub quote_bits: u8,
    /// `--omit-empty`: no newline after a ref whose formatted output is empty.
    pub omit_empty: bool,
    /// `--count=<n>`: stop after `n` refs (0 = all); negative values are rejected.
    pub count: i64,
    /// `--format=<format>` (default `%(objectname) %(objecttype)\t%(refname)`).
    pub format: String,
    /// `--color[=<when>]`: `Some(true/false)` when forced, `None` for auto (`color.ui`, tty).
    pub color: Option<bool>,
    /// `--exclude=<pattern>`, `--sort=<key>` (last given is the primary key).
    pub excludes: Vec<String>,
    pub sorts: Vec<String>,
    /// `--points-at=<object>`: object ids a ref (or one of its tag peels) must name.
    pub points_at: Vec<String>,
    /// `--merged` / `--no-merged` / `--contains` / `--no-contains`: commit ids.
    pub merged: Vec<String>,
    pub no_merged: Vec<String>,
    pub contains: Vec<String>,
    pub no_contains: Vec<String>,
    /// `--ignore-case`: case-insensitive pattern matching and sorting.
    pub ignore_case: bool,
    /// `--stdin`: read the patterns from standard input.
    pub stdin: bool,
    pub patterns: Vec<String>,
}

/// A `for-each-ref` failure carrying git's exit code and what to print on each stream.
// Used by: src/references/for_each_ref.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_for_each_ref_args`, `prepare_ref_format` and the `for_each_ref` command
#[derive(Debug, Clone, Default)]
pub struct ForEachRefExit {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The field a `%(atom)` in a `for-each-ref` format or `--sort` key names (git's `valid_atom`).
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_atom` and `populate_ref_values`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefAtomKind {
    Refname,
    ObjectType,
    ObjectSize,
    ObjectName,
    DeltaBase,
    Tree,
    Parent,
    NumParent,
    Object,
    Type,
    Tag,
    Author,
    AuthorName,
    AuthorEmail,
    AuthorDate,
    Committer,
    CommitterName,
    CommitterEmail,
    CommitterDate,
    Tagger,
    TaggerName,
    TaggerEmail,
    TaggerDate,
    Creator,
    CreatorDate,
    Describe,
    Subject,
    Body,
    Trailers,
    Contents,
    Signature,
    Raw,
    Upstream,
    Push,
    Symref,
    Flag,
    Head,
    Color,
    WorktreePath,
    Align,
    End,
    If,
    Then,
    Else,
    Rest,
    AheadBehind,
}

/// How a ref name is shown by `%(refname)`, `%(symref)`, `%(upstream)` and `%(push)`: in full,
/// `:short` (shortest unambiguous), `:lstrip=<n>` or `:rstrip=<n>` (negative = keep `-n`).
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_atom` and `show_ref_name`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefnameShow {
    Full,
    Short,
    Lstrip(i64),
    Rstrip(i64),
}

/// What `%(upstream:...)` / `%(push:...)` prints: the ref, `track` (`[ahead 1, behind 2]`),
/// `trackshort` (`<>`), `remotename` or `remoteref`.
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_atom` and `remote_ref_details`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteRefShow {
    Ref,
    Track,
    TrackShort,
    RemoteName,
    RemoteRef,
}

/// Which part of a commit or tag message `%(subject)`, `%(body)`, `%(trailers)` and
/// `%(contents:...)` print.
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_atom` and `grab_message_values`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefContentsPart {
    Bare,
    Body,
    BodyWithSignature,
    Size,
    Lines(usize),
    Signature,
    Subject,
    SubjectSanitized,
    Trailers(TrailerFormatOptions),
}

/// Options of `%(trailers:...)`: `only`, `unfold`, `keyonly`, `valueonly`, `key=<k>` (repeatable,
/// implies `only`), `separator=<s>` and `key_value_separator=<s>` (with `%n`/`%xNN` expanded).
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_trailer_options` and `format_message_trailers`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrailerFormatOptions {
    pub only: bool,
    pub unfold: bool,
    pub key_only: bool,
    pub value_only: bool,
    pub keys: Vec<String>,
    pub separator: Option<Vec<u8>>,
    pub key_value_separator: Option<Vec<u8>>,
}

/// Where `%(align:...)` puts its text inside the padded width.
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_atom` and `align_ref_text`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefAlign {
    Left,
    Middle,
    Right,
}

/// An atom's parsed argument (the part after `:`), per atom kind.
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_atom` and `populate_ref_values`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefAtomArg {
    None,
    Refname(RefnameShow),
    RemoteRef { show: RemoteRefShow, refname: RefnameShow, nobracket: bool, push: bool, push_remote: bool },
    /// `None` = full hash, `Some(n)` = abbreviated to at least `n` digits.
    Oid(Option<usize>),
    ObjectSize { disk: bool },
    Contents(RefContentsPart),
    Email { trim: bool, localpart: bool },
    Describe { tags: Option<bool>, abbrev: Option<usize>, matches: Vec<String>, excludes: Vec<String> },
    Raw { size: bool },
    /// The escape sequence to emit (empty when colors are off).
    Color(String),
    Align { position: RefAlign, width: usize },
    /// `%(if:equals=<s>)` is `Some((true, s))`, `notequals` is `Some((false, s))`.
    If(Option<(bool, String)>),
    Signature(String),
    /// The ref HEAD resolves to, when it does.
    Head(Option<String>),
    /// The commit `%(ahead-behind:<committish>)` counts against.
    AheadBehind(String),
}

/// One distinct atom used by a `for-each-ref` format or sort key (git's `used_atom`).
// Used by: src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_atom`, `populate_ref_values`, `format_ref_item` and `compare_ref_items`
#[derive(Debug, Clone)]
pub struct RefFilterAtom {
    pub kind: RefAtomKind,
    /// The text between `%(` and `)`, including a leading `*`.
    pub name: String,
    /// `%(*atom)`: read the value from the object an annotated tag points at.
    pub deref: bool,
    /// Sorted by number (sizes, dates, parent counts) rather than as a string.
    pub numeric: bool,
    /// Whether the value comes from the object's content (git's `SOURCE_OBJ`).
    pub needs_content: bool,
    pub arg: RefAtomArg,
}

/// A ref selected by `for-each-ref`, before its values are computed.
// Used by: src/references/for_each_ref/engine.rs, src/workspace/branch/engine.rs
// Used by command/helper: `collect_ref_items`, `filter_ref_items` and `populate_ref_values`
#[derive(Debug, Clone)]
pub struct RefArrayItem {
    pub refname: String,
    pub oid: String,
    /// The ref is a symbolic ref (`ref: ...`), and/or is read from `packed-refs`.
    pub symref: bool,
    pub packed: bool,
}

/// An atom's value for one ref: the bytes to print and, for numeric atoms, the sort value.
/// `binary` marks raw object data, which is compared by length instead of up to a NUL.
// Used by: src/references/for_each_ref/engine.rs
// Used by command/helper: `populate_ref_values`, `format_ref_item` and `compare_ref_items`
#[derive(Debug, Clone, Default)]
pub struct RefAtomValue {
    pub s: Vec<u8>,
    pub value: u64,
    pub binary: bool,
}

/// A `--date`-style mode for `%(authordate:<mode>)` and friends: git's `relative`, `iso`,
/// `iso-strict`, `rfc`, `short`, `default`, `human`, `raw`, `unix` and `format:<strftime>`,
/// each optionally `-local`.
// Used by: src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_date_mode` and `show_ref_date`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefDateMode {
    pub kind: RefDateKind,
    pub local: bool,
    pub strftime: String,
}

/// The base kind of a [`RefDateMode`].
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `parse_ref_date_mode` and `show_ref_date`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RefDateKind {
    #[default]
    Normal,
    Relative,
    Iso,
    IsoStrict,
    Rfc,
    Short,
    Human,
    Raw,
    Unix,
    Strftime,
}

/// One piece of a parsed `for-each-ref` format: literal text (with `%%` / `%xx` still to
/// expand) or the index of an atom in the parsed atom list.
// Used by: src/references/for_each_ref/types.rs, src/references/for_each_ref/engine.rs
// Used by command/helper: `prepare_ref_format` and `format_ref_item`
pub type RefFormatPiece = (Option<String>, Option<usize>);
