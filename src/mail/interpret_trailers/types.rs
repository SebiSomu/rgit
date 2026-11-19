/// Where `interpret-trailers` puts a new trailer relative to the existing ones
/// (`--where` / `trailer.where` / `trailer.<token>.where`).
// Used by: src/mail/interpret_trailers/types.rs, src/mail/interpret_trailers/engine.rs
// Used by command/helper: `parse_interpret_trailers_args`, `read_trailer_config`, and `process_trailers`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrailerWhere {
    /// Not given on the command line: use the configured placement.
    #[default]
    Default,
    /// After all existing trailers (git's default).
    End,
    /// Right after the last trailer with the same token.
    After,
    /// Right before the first trailer with the same token.
    Before,
    /// Before all existing trailers.
    Start,
}

/// What `interpret-trailers` does with a new trailer whose token already exists in the
/// message (`--if-exists` / `trailer.ifexists` / `trailer.<token>.ifexists`).
// Used by: src/mail/interpret_trailers/types.rs, src/mail/interpret_trailers/engine.rs
// Used by command/helper: `parse_interpret_trailers_args`, `read_trailer_config`, and `process_trailers`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrailerIfExists {
    /// Not given on the command line: use the configured action.
    #[default]
    Default,
    /// Add unless the neighbouring trailer has the same token and value (git's default).
    AddIfDifferentNeighbor,
    /// Add unless some trailer has the same token and value.
    AddIfDifferent,
    /// Always add.
    Add,
    /// Replace the existing trailer with the new one.
    Replace,
    /// Leave the message alone.
    DoNothing,
}

/// What `interpret-trailers` does with a new trailer whose token isn't in the message yet
/// (`--if-missing` / `trailer.ifmissing` / `trailer.<token>.ifmissing`).
// Used by: src/mail/interpret_trailers/types.rs, src/mail/interpret_trailers/engine.rs
// Used by command/helper: `parse_interpret_trailers_args`, `read_trailer_config`, and `process_trailers`
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrailerIfMissing {
    /// Not given on the command line: use the configured action.
    #[default]
    Default,
    /// Leave the message alone.
    DoNothing,
    /// Add the trailer (git's default).
    Add,
}

/// The settings of one configured trailer (`trailer.<name>.*`), or the defaults when `name`
/// is empty: its canonical `key`, an optional `command` / `cmd` that computes its value, and
/// where and when it is added.
// Used by: src/mail/interpret_trailers/types.rs, src/mail/interpret_trailers/engine.rs
// Used by command/helper: `read_trailer_config` and `process_trailers`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrailerConf {
    pub name: String,
    pub key: Option<String>,
    pub command: Option<String>,
    pub cmd: Option<String>,
    pub place: TrailerWhere,
    pub if_exists: TrailerIfExists,
    pub if_missing: TrailerIfMissing,
}

/// The whole `trailer.*` configuration: the separator characters (`trailer.separators`,
/// `:` by default), the default settings, and the configured trailers in config order.
// Used by: src/mail/interpret_trailers/engine.rs
// Used by command/helper: `read_trailer_config`, `process_trailers`, and the `interpret_trailers` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrailerConfig {
    pub separators: String,
    pub default: TrailerConf,
    pub items: Vec<TrailerConf>,
}

/// One line of a message's trailer block: a trailer (`token` and `value`), or, when `token`
/// is `None`, a non-trailer line kept as it is in `value`.
// Used by: src/mail/interpret_trailers/engine.rs
// Used by command/helper: `process_trailers` and its list-editing helpers
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrailerItem {
    pub token: Option<Vec<u8>>,
    pub value: Vec<u8>,
}

/// A trailer about to be added to a message (from `--trailer` or from a configured
/// `command`), with the settings that decide where and whether it goes in.
// Used by: src/mail/interpret_trailers/engine.rs
// Used by command/helper: `process_trailers` and its list-editing helpers
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrailerArg {
    pub token: Vec<u8>,
    pub value: Vec<u8>,
    pub conf: TrailerConf,
}

/// One `--trailer <token>[(=|:)<value>]` argument together with the `--where`,
/// `--if-exists` and `--if-missing` options that were in effect when it was given.
// Used by: src/mail/interpret_trailers/types.rs, src/mail/interpret_trailers/engine.rs
// Used by command/helper: `parse_interpret_trailers_args` and `process_trailers`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewTrailer {
    pub text: String,
    pub place: TrailerWhere,
    pub if_exists: TrailerIfExists,
    pub if_missing: TrailerIfMissing,
}

/// Parsed arguments for the `interpret-trailers` command, which adds or parses the
/// `Token: value` lines at the end of commit messages.
// Used by: src/mail/interpret_trailers/engine.rs
// Used by command/helper: `parse_interpret_trailers_args`, `process_trailers`, and the `interpret_trailers` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InterpretTrailersOptions {
    /// `--in-place`: rewrite the input files instead of printing the result.
    pub in_place: bool,
    /// `--trim-empty`: drop trailers whose value is empty.
    pub trim_empty: bool,
    /// `--only-trailers`: print only the trailers, not the rest of the message.
    pub only_trailers: bool,
    /// `--only-input`: don't add trailers from `--trailer` or configured commands.
    pub only_input: bool,
    /// `--unfold`: join multi-line trailer values into one line.
    pub unfold: bool,
    /// `--no-divider`: don't treat a `---` line as the end of the message.
    pub no_divider: bool,
    /// The `--trailer` arguments, in order.
    pub trailers: Vec<NewTrailer>,
    /// Message files to process; standard input when empty.
    pub files: Vec<String>,
}
