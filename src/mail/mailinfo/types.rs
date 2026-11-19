/// What `mailinfo` does when a base64 / quoted-printable part decodes to lines ending in CRLF.
// Used by: src/mail/mailinfo/types.rs, src/mail/mailinfo/engine.rs
// Used by command/helper: `parse_mailinfo_args` and `extract_mail_info` (for `mailinfo --quoted-cr`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QuotedCrAction {
    /// `nowarn`: keep the CRs silently.
    NoWarn,
    /// `warn` (default): keep the CRs and print `warning: quoted CRLF detected`.
    #[default]
    Warn,
    /// `strip`: turn CRLF into LF.
    Strip,
}

/// The `Content-Transfer-Encoding` of the mail part `mailinfo` is currently reading.
// Used by: src/mail/mailinfo/types.rs, src/mail/mailinfo/engine.rs
// Used by command/helper: `extract_mail_info` and its body decoding helpers
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MailTransferEncoding {
    /// 7bit, 8bit, binary or unknown: lines are used as they are.
    #[default]
    DontCare,
    /// `quoted-printable`: `=XX` escapes and soft line breaks.
    QuotedPrintable,
    /// `base64`.
    Base64,
}

/// Parsed arguments for the `mailinfo` command, which separates one email into commit
/// message, patch, and author/subject/date summary.
// Used by: src/mail/mailinfo/engine.rs
// Used by command/helper: `parse_mailinfo_args`, `extract_mail_info`, and the `mailinfo` command
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MailinfoOptions {
    /// `-k`: keep the subject as it is (no `Re:` / `[PATCH]` stripping, no whitespace cleanup).
    pub keep_subject: bool,
    /// `-b`: when cleaning the subject, only remove brackets that contain `PATCH`.
    pub keep_non_patch_brackets: bool,
    /// `-m` / `--message-id`: append the mail's `Message-ID:` to the commit message.
    pub add_message_id: bool,
    /// Encoding that names, subject and message are re-coded to (`-u` / `--encoding`); `None` with `-n`.
    pub metainfo_charset: Option<String>,
    /// `--scissors`: drop everything before a scissors line (`-- >8 --`) in the body.
    pub use_scissors: bool,
    /// Off with `--no-inbody-headers`: don't let `From:`/`Subject:`/`Date:` lines in the body override the mail headers.
    pub use_inbody_headers: bool,
    /// `--quoted-cr=<action>`: what to do with CRLF found in decoded parts.
    pub quoted_cr: QuotedCrAction,
    /// `<msg>`: file that receives the commit message.
    pub msg_file: String,
    /// `<patch>`: file that receives the patch.
    pub patch_file: String,
}

/// The working state of one `mailinfo` run, mirroring git's `struct mailinfo`: the input
/// and read position, the collected mail and in-body headers, the MIME decoding state of
/// the current part, and the message and patch built so far.
// Used by: src/mail/mailinfo/engine.rs
// Used by command/helper: `extract_mail_info` and its header/body parsing helpers
#[derive(Debug, Clone, Default)]
pub struct MailinfoState {
    /// The whole email, the offset of the next unread byte, and whether a read has already
    /// run into the end of the input (stdio's `feof`, which decides what a failed read leaves behind).
    pub input: Vec<u8>,
    pub pos: usize,
    pub eof: bool,
    /// `From`, `Subject` and `Date` from the mail headers (`p_hdr`) and from the body (`s_hdr`).
    pub p_hdr: [Option<Vec<u8>>; 3],
    pub s_hdr: [Option<Vec<u8>>; 3],
    /// Stack of MIME multipart boundary markers (each with its leading `--`), innermost last.
    pub boundaries: Vec<Vec<u8>>,
    /// Charset and transfer encoding of the part being read, and its `format=flowed` / `delsp=yes` flags.
    pub charset: Vec<u8>,
    pub transfer_encoding: MailTransferEncoding,
    pub format_flowed: bool,
    pub delsp: bool,
    /// A decoded line of the current part ended in CRLF.
    pub have_quoted_cr: bool,
    /// False while lines belong to the commit message, true once the patch has started.
    pub in_patch: bool,
    /// Still at the top of the body, where blank lines are skipped and in-body headers may appear.
    pub header_stage: bool,
    /// An in-body header being collected (it may continue on the next line).
    pub inbody_header_accum: Vec<u8>,
    /// Outputs: the commit message, the patch and its line count, and the `Message-ID` (with `-m`).
    pub log_message: Vec<u8>,
    pub patch: Vec<u8>,
    pub patch_lines: usize,
    pub message_id: Option<Vec<u8>>,
    /// Author name and email taken from the `From` header.
    pub name: Vec<u8>,
    pub email: Vec<u8>,
    /// Something could not be decoded; `mailinfo` exits 1.
    pub input_error: bool,
}

/// What `mailinfo` produces from one email: the commit message, the patch, the summary
/// printed on standard output, and whether the mail was empty or could not be decoded.
// Used by: src/mail/mailinfo/engine.rs
// Used by command/helper: `extract_mail_info` and the `mailinfo` command
#[derive(Debug, Clone, Default)]
pub struct MailinfoOutput {
    pub message: Vec<u8>,
    pub patch: Vec<u8>,
    pub info: Vec<u8>,
    pub failed: bool,
}
