/// The corrected name and/or email a mailmap line gives for a commit identity; a `None`
/// part is left as it was.
// Used by: src/mail/mailmap/types.rs, src/mail/mailmap.rs
// Used by command/helper: `add_mailmap_line` and `map_user`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailmapInfo {
    pub name: Option<String>,
    pub email: Option<String>,
}

/// Everything the mailmap says about one commit email: the mapping that applies whatever
/// the commit name is (`simple`), and the mappings that only apply to a specific commit
/// name (`by_name`, keyed by that name).
// Used by: src/mail/mailmap/types.rs, src/mail/mailmap.rs
// Used by command/helper: `add_mailmap_line` and `map_user`
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailmapEntry {
    pub old_email: String,
    pub simple: MailmapInfo,
    pub by_name: Vec<(String, MailmapInfo)>,
}

/// A parsed mailmap (`.mailmap`, `mailmap.file`, `mailmap.blob`): one entry per commit
/// email, compared ignoring case.
// Used by: src/mail/check_mailmap.rs, src/mail/mailmap.rs, src/mail/request_pull/engine.rs
// Used by command/helper: `read_mailmap`, `map_user`, and the `check_mailmap` / `request_pull` commands
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mailmap {
    pub entries: Vec<MailmapEntry>,
}

/// Parsed arguments for the `check-mailmap` command, which shows the canonical name and
/// email of contacts.
// Used by: src/mail/mailmap.rs
// Used by command/helper: `parse_check_mailmap_args` and the `check_mailmap` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckMailmapOptions {
    /// `--stdin`: also read contacts from standard input, one per line.
    pub stdin: bool,
    /// `--mailmap-file <file>`: extra mailmap file, read last.
    pub mailmap_file: Option<String>,
    /// `--mailmap-blob <blob>`: extra mailmap blob (e.g. `HEAD:.mailmap`).
    pub mailmap_blob: Option<String>,
    /// The contacts to look up: `Name <email>` or `<email>`.
    pub contacts: Vec<String>,
}
