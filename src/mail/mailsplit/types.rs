/// Parsed arguments for the `mailsplit` command, which cuts mbox files (or Maildirs) into
/// one numbered file per message.
// Used by: src/mail/mailsplit.rs, src/mail/mailsplit/engine.rs
// Used by command/helper: `parse_mailsplit_args`, `split_mbox_messages`, `first_maildir_message`, and the `mailsplit` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailsplitOptions {
    /// `-o<directory>`: where the numbered message files are written.
    pub directory: String,
    /// `-d<prec>`: digits in the file names (default 4, allowed 3 to 9).
    pub precision: usize,
    /// `-f<n>`: number of messages already split; numbering starts at `n + 1`.
    pub skip: usize,
    /// `-b`: accept a mailbox with a single message that lacks the `From ` separator line.
    pub allow_bare: bool,
    /// `--keep-cr`: keep `\r\n` line endings instead of turning them into `\n`.
    pub keep_cr: bool,
    /// `--mboxrd`: remove one `>` from lines quoted as `>From `.
    pub mboxrd: bool,
    /// The mbox files / Maildir directories to split; `-` is standard input.
    pub inputs: Vec<String>,
}
