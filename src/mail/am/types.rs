use crate::core::types::CommitSignature;

/// Options for the `am` command, which applies a mailbox of patches as commits.
// Used by: src/cli.rs, src/mail/am.rs
// Used by command/helper: the `am` command (input, resume actions, and subject handling)
#[derive(Debug, Clone, Default)]
pub struct AmOptions {
    /// Mailbox files (or `format-patch` files) to read; standard input when empty.
    pub mboxes: Vec<String>,
    /// `--continue` / `-r` / `--resolved`: commit the resolved index for the stopped patch and go on.
    pub cont: bool,
    /// `--skip`: drop the stopped patch and go on with the rest.
    pub skip: bool,
    /// `--abort`: restore the branch, index, and working tree to where `am` started.
    pub abort: bool,
    /// `--quit`: forget the session but keep HEAD, index, and working tree as they are.
    pub quit: bool,
    /// `--show-current-patch[=raw|diff]`: print the stopped patch's mail or just its diff.
    pub show_current_patch: Option<String>,
    /// `-k` / `--keep-subject`: keep `[PATCH]` and other bracketed prefixes in the subject.
    pub keep_subject: bool,
}

/// A patch email decoded by `am`: author identity and date (from `From:`/`Date:`, possibly
/// overridden by in-body headers), the cleaned subject, the final commit message, and the
/// diff text that follows the `---` separator.
// Used by: src/mail/am.rs, src/mail/am/engine.rs
// Used by command/helper: `parse_mail`, `commit_mail`, and the `am` command
#[derive(Debug, Clone, Default)]
pub struct MailPatch {
    pub author: CommitSignature,
    pub subject: String,
    pub message: String,
    pub patch: String,
}
