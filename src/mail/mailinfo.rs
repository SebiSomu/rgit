pub mod engine;

use std::fs;
use std::io::{self, Read, Write};
use anyhow::Result;

use crate::mail::mailinfo::engine::{extract_mail_info, parse_mailinfo_args, MAILINFO_USAGE};

/// `rgit mailinfo [-k] [-b] [-m | --message-id] [-u | -n | --encoding=<encoding>]
/// [--[no-]scissors] [--no-inbody-headers] [--quoted-cr=<action>] <msg> <patch>`
///
/// Reads a single email from standard input (one file written by `mailsplit`), writes its
/// commit message to `<msg>` and its patch to `<patch>`, and prints the author name, email,
/// subject and date on standard output as `Author:` / `Email:` / `Subject:` / `Date:` lines
/// followed by a blank line. This is the parser behind `am`. Headers are unfolded and RFC
/// 2047-decoded; `From:`, `Subject:` and `Date:` lines at the top of the body override the
/// mail's own; `[PATCH n/m]`-style prefixes and `Re:` are removed from the subject unless
/// `-k` (`-b` only removes brackets that mention PATCH). MIME multipart mails, base64 and
/// quoted-printable bodies and `format=flowed` text are decoded, and text is re-coded to
/// UTF-8 (`--encoding` picks another target, `-n` turns re-coding off). `--scissors` drops
/// everything before a `-- >8 --` line, `-m` appends the mail's `Message-ID:` to the
/// message, and `--quoted-cr=nowarn|warn|strip` decides what happens to CRLF line endings
/// found inside encoded parts. Works outside a repository. Exit status, like git: 0
/// success, 1 an empty or undecodable mail, 128 fatal, 129 usage.
pub fn mailinfo(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h" || a == "--help") {
        print!("{}", MAILINFO_USAGE);
        crate::ui::exit(129);
    }
    let opts = match parse_mailinfo_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            if message.starts_with("fatal:") {
                eprintln!("{}", message);
                crate::ui::exit(128);
            }
            eprint!("{}", message);
            crate::ui::exit(129);
        }
    };

    let mut input = Vec::new();
    io::stdin().lock().read_to_end(&mut input)?;
    let result = extract_mail_info(input, &opts);

    for (path, content) in [(&opts.msg_file, &result.message), (&opts.patch_file, &result.patch)] {
        if let Err(e) = fs::write(path, content) {
            eprintln!("{}: {}", path, e);
            crate::ui::exit(1);
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    out.write_all(&result.info)?;
    out.flush()?;
    if result.failed {
        crate::ui::exit(1);
    }
    Ok(())
}
