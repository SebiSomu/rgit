use std::cmp::Ordering;
use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::mail::mailsplit::types::MailsplitOptions;

/// git's usage text for `mailsplit`, printed for `-h` and after argument errors (exit 129).
pub const MAILSPLIT_USAGE: &str = "usage: rgit mailsplit [-d<prec>] [-f<n>] [-b] [--keep-cr] [--mboxrd] -o<directory> [(<mbox>|<Maildir>)...]\n";

/// Parses `mailsplit` arguments like git's hand-written loop: flags come first and take
/// their value glued on (`-o<dir>`, `-d<prec>`, `-f<n>`), `--` ends them, and without `-o`
/// the old forms `<mbox> <directory>` and `<directory>` (reading standard input) are
/// accepted. Errors starting with `usage:` mean exit 129, others are fatal (128).
// Used by `mailsplit`.
pub fn parse_mailsplit_args(args: &[String]) -> anyhow::Result<MailsplitOptions> {
    let mut opts = MailsplitOptions { precision: 4, ..Default::default() };
    let mut directory: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if !arg.starts_with('-') {
            break;
        }
        if let Some(value) = arg.strip_prefix("-d") {
            opts.precision = leading_number(value) as usize;
            if !(3..10).contains(&leading_number(value)) {
                anyhow::bail!("{}", MAILSPLIT_USAGE);
            }
        } else if let Some(value) = arg.strip_prefix("-f") {
            opts.skip = leading_number(value).max(0) as usize;
        } else if arg.starts_with("-o") && arg.len() > 2 {
            directory = Some(arg[2..].to_string());
        } else if arg == "-b" {
            opts.allow_bare = true;
        } else if arg == "--keep-cr" {
            opts.keep_cr = true;
        } else if arg == "--mboxrd" {
            opts.mboxrd = true;
        } else if arg == "--" {
            i += 1;
            break;
        } else {
            anyhow::bail!("fatal: unknown option: {}", arg);
        }
        i += 1;
    }

    let rest = &args[i..];
    match directory {
        Some(dir) => {
            opts.directory = dir;
            opts.inputs = if rest.is_empty() { vec!["-".to_string()] } else { rest.to_vec() };
        }
        None => match rest {
            [dir] => {
                opts.directory = dir.clone();
                opts.inputs = vec!["-".to_string()];
            }
            [mbox, dir] => {
                opts.directory = dir.clone();
                opts.inputs = vec![mbox.clone()];
            }
            _ => anyhow::bail!("{}", MAILSPLIT_USAGE),
        },
    }
    Ok(opts)
}

/// C `strtol` on the start of `text`: optional sign and digits, 0 when there are none.
// Used by `parse_mailsplit_args` and `is_mbox_from_line`.
fn leading_number(text: &str) -> i64 {
    let text = text.trim_start();
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    sign * digits[..end].parse::<i64>().unwrap_or(0)
}

/// Port of git's `is_from_line`: an mbox separator starts with `From ` and ends with
/// something that looks like a time and a year (`... HH:MM:SS YYYY`), judged loosely by the
/// digits around the last colon and a year above 90.
// Used by `mailsplit` via `split_mbox_messages`.
pub fn is_mbox_from_line(line: &[u8]) -> bool {
    let len = line.len();
    if len < 20 || !line.starts_with(b"From ") {
        return false;
    }
    let mut colon = len - 2;
    loop {
        if colon < 5 {
            return false;
        }
        colon -= 1;
        if line[colon] == b':' {
            break;
        }
    }
    let digit = |i: usize| line.get(i).is_some_and(|b| b.is_ascii_digit());
    if !digit(colon - 4) || !digit(colon - 2) || !digit(colon + 1) || !digit(colon + 2) {
        return false;
    }
    leading_number(&String::from_utf8_lossy(&line[(colon + 3).min(len)..])) > 90
}

/// Splits mbox bytes into messages like git's `split_mbox` / `split_one`: leading blank
/// space is skipped, every `From ` separator line starts a new message (and stays as its
/// first line), CRs before LFs are dropped unless `keep_cr`, and with `mboxrd` one `>` is
/// removed from `>From ` quoting. A mailbox that doesn't start with a separator is one
/// "bare" message when `allow_bare` and "corrupt mailbox" (an error) otherwise. Returns
/// `None` for input that is empty or only whitespace.
// Used by `mailsplit`.
pub fn split_mbox_messages(data: &[u8], opts: &MailsplitOptions) -> anyhow::Result<Option<Vec<Vec<u8>>>> {
    let start = data.iter().position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r')).unwrap_or(data.len());
    if start == data.len() {
        return Ok(None);
    }
    Ok(Some(split_message_lines(&data[start..], opts, opts.allow_bare)?))
}

/// Extracts the single message of a Maildir file like git's `split_maildir`: the whole file,
/// or, when it starts with an mbox `From ` separator, everything up to the next separator.
// Used by `mailsplit`.
pub fn first_maildir_message(data: &[u8], opts: &MailsplitOptions) -> Vec<u8> {
    split_message_lines(data, opts, true).ok().and_then(|messages| messages.into_iter().next()).unwrap_or_default()
}

/// Port of the loop around git's `split_one`: cuts lines into messages at `From ` separator
/// lines, applying the `--keep-cr` and `--mboxrd` line rewriting.
// Used by `split_mbox_messages` and `first_maildir_message`.
fn split_message_lines(data: &[u8], opts: &MailsplitOptions, allow_bare: bool) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut lines = data.split_inclusive(|&b| b == b'\n').peekable();
    let mut messages = Vec::new();

    while let Some(first) = lines.next() {
        let is_bare = !is_mbox_from_line(first);
        if is_bare && !allow_bare {
            anyhow::bail!("corrupt mailbox");
        }
        let mut message = Vec::new();
        let mut line = first;
        loop {
            let mut text = line.to_vec();
            if !opts.keep_cr && text.len() > 1 && text.ends_with(b"\r\n") {
                text.truncate(text.len() - 2);
                text.push(b'\n');
            }
            if opts.mboxrd && is_quoted_from(&text) {
                text.remove(0);
            }
            message.extend_from_slice(&text);
            match lines.peek() {
                Some(next) if !is_bare && is_mbox_from_line(next) => break,
                Some(_) => line = lines.next().unwrap(),
                None => break,
            }
        }
        messages.push(message);
    }
    Ok(messages)
}

/// git's `is_gtfrom`: a line of one or more `>` followed by `From ` (mboxrd quoting).
// Used by `split_message_lines`.
fn is_quoted_from(line: &[u8]) -> bool {
    let quotes = line.iter().take_while(|&&b| b == b'>').count();
    quotes > 0 && line[quotes..].starts_with(b"From ")
}

/// Lists the mails of a Maildir (`cur/` then `new/`, skipping dot files) as paths relative
/// to it, sorted like git's `maildir_filename_cmp`: runs of digits compare as numbers so
/// `2.mail` comes before `10.mail`.
// Used by `mailsplit`.
pub fn list_maildir_files(maildir: &Path) -> anyhow::Result<Vec<String>> {
    let mut files = Vec::new();
    for sub in ["cur", "new"] {
        let dir = maildir.join(sub);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("error: cannot opendir {}", dir.display())),
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') {
                files.push(format!("{}/{}", sub, name));
            }
        }
    }
    files.sort_by(|a, b| maildir_filename_cmp(a.as_bytes(), b.as_bytes()));
    files.dedup();
    Ok(files)
}

/// Port of git's `maildir_filename_cmp`: byte order, except that digit runs are compared
/// by numeric value.
// Used by `list_maildir_files`.
fn maildir_filename_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let take = |s: &[u8], at: &mut usize| -> u128 {
                let mut n: u128 = 0;
                while *at < s.len() && s[*at].is_ascii_digit() {
                    n = n.saturating_mul(10).saturating_add((s[*at] - b'0') as u128);
                    *at += 1;
                }
                n
            };
            let (na, nb) = (take(a, &mut i), take(b, &mut j));
            if na != nb {
                return na.cmp(&nb);
            }
        } else {
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    a.get(i).copied().unwrap_or(0).cmp(&b.get(j).copied().unwrap_or(0))
}

/// Writes one split message as `<directory>/<number>` (zero-padded to `precision` digits),
/// refusing to overwrite an existing file, like git's `O_EXCL` open.
// Used by `mailsplit`.
pub fn write_split_message(directory: &str, precision: usize, number: usize, message: &[u8]) -> anyhow::Result<()> {
    use std::io::Write;
    let path = format!("{}/{:0width$}", directory, number, width = precision);
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| {
        let reason = match e.kind() {
            std::io::ErrorKind::AlreadyExists => "File exists".to_string(),
            std::io::ErrorKind::NotFound => "No such file or directory".to_string(),
            std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
            _ => e.to_string(),
        };
        anyhow::anyhow!("fatal: unable to create '{}': {}", path, reason)
    })?;
    file.write_all(message).with_context(|| "fatal: cannot write output".to_string())?;
    Ok(())
}

