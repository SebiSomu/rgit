use crate::core::config::get_config_value;
use crate::mail::mailinfo::types::{MailinfoOptions, MailinfoOutput, MailinfoState, MailTransferEncoding, QuotedCrAction};

/// git's usage text for `mailinfo`, printed for `-h` and after argument errors (exit 129).
pub const MAILINFO_USAGE: &str = "usage: rgit mailinfo [<options>] <msg> <patch> < mail >info

    -k                    keep subject
    -b                    keep non patch brackets in subject
    -m, --message-id      copy Message-ID to the end of commit message
    -u                    re-code metadata to i18n.commitEncoding
    -n                    disable charset re-coding of metadata
    --encoding <encoding> re-code metadata to this encoding
    --scissors            use scissors
    --quoted-cr <action>  action when quoted CR is found
    --no-inbody-headers   use headers in message's body
";

/// The mail headers `mailinfo` extracts, in the order git reports them.
const MAIL_HEADERS: [&str; 3] = ["From", "Subject", "Date"];

/// Most nested MIME boundaries followed at once (git's `MAX_BOUNDARIES` minus the base slot).
const MAX_BOUNDARIES: usize = 4;

/// git's `isspace`: space, tab, LF or CR.
// Used by `mailinfo` (internal to this file's helpers).
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Position of the first occurrence of `needle` in `haystack`.
// Used by `mailinfo` (internal to this file's helpers).
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Position of the first occurrence of `needle` in `haystack`, ignoring ASCII case.
// Used by `mailinfo` (internal to this file's helpers).
fn find_bytes_ignore_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w.eq_ignore_ascii_case(needle))
}

/// The part of `buf` before its first NUL byte, which is all C's `%s` would print.
// Used by `mailinfo` (internal to this file's helpers).
fn until_nul(buf: &[u8]) -> &[u8] {
    &buf[..buf.iter().position(|&b| b == 0).unwrap_or(buf.len())]
}

/// git's `strbuf_trim`: drops leading and trailing whitespace.
// Used by `mailinfo` (internal to this file's helpers).
fn trim_in_place(buf: &mut Vec<u8>) {
    while buf.last().is_some_and(|&b| is_space(b)) {
        buf.pop();
    }
    let start = buf.iter().position(|&b| !is_space(b)).unwrap_or(buf.len());
    buf.drain(..start);
}

/// Parses `mailinfo` arguments: the switches git accepts in any position and exactly two
/// file names (`<msg>` and `<patch>`). Defaults follow git: metadata is re-coded to
/// `i18n.commitEncoding` (UTF-8), scissors come from `mailinfo.scissors`, and quoted CRs
/// warn unless `mailinfo.quotedCr` says otherwise. Errors carry the usage text (exit 129),
/// except a bad `--quoted-cr` action, which is fatal.
// Used by `mailinfo`.
pub fn parse_mailinfo_args(args: &[String]) -> anyhow::Result<MailinfoOptions> {
    let config = |key: &str| get_config_value(key).ok().flatten();
    let truthy = |v: String| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1");
    let default_charset = config("i18n.commitencoding").filter(|v| !v.is_empty()).unwrap_or_else(|| "UTF-8".to_string());

    let mut opts = MailinfoOptions {
        metainfo_charset: Some(default_charset.clone()),
        use_scissors: config("mailinfo.scissors").is_some_and(truthy),
        use_inbody_headers: true,
        quoted_cr: QuotedCrAction::Warn,
        ..Default::default()
    };
    if let Some(action) = config("mailinfo.quotedcr") {
        opts.quoted_cr = parse_quoted_cr_action(&action)?;
    }

    let mut files = Vec::new();
    let mut i = 0;
    let mut only_files = false;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if only_files || !arg.starts_with('-') || arg == "-" {
            files.push(arg.to_string());
            continue;
        }
        match arg {
            "--" => only_files = true,
            "-k" => opts.keep_subject = true,
            "-b" => opts.keep_non_patch_brackets = true,
            "-m" | "--message-id" => opts.add_message_id = true,
            "--no-message-id" => opts.add_message_id = false,
            "-u" => opts.metainfo_charset = Some(default_charset.clone()),
            "-n" => opts.metainfo_charset = None,
            "--scissors" => opts.use_scissors = true,
            "--no-scissors" => opts.use_scissors = false,
            "--no-inbody-headers" => opts.use_inbody_headers = false,
            "--inbody-headers" => opts.use_inbody_headers = true,
            "--encoding" | "--quoted-cr" => {
                let Some(value) = args.get(i) else {
                    anyhow::bail!("error: option `{}' requires a value\n", &arg[2..]);
                };
                i += 1;
                if arg == "--encoding" {
                    opts.metainfo_charset = Some(value.clone());
                } else {
                    opts.quoted_cr = parse_quoted_cr_action(value)?;
                }
            }
            _ => {
                if let Some(value) = arg.strip_prefix("--encoding=") {
                    opts.metainfo_charset = Some(value.to_string());
                } else if let Some(value) = arg.strip_prefix("--quoted-cr=") {
                    opts.quoted_cr = parse_quoted_cr_action(value)?;
                } else if arg.starts_with("--") {
                    anyhow::bail!("error: unknown option `{}'\n{}", &arg[2..], MAILINFO_USAGE);
                } else {
                    // Bundled short switches such as `-kb`.
                    for c in arg[1..].chars() {
                        match c {
                            'k' => opts.keep_subject = true,
                            'b' => opts.keep_non_patch_brackets = true,
                            'm' => opts.add_message_id = true,
                            'u' => opts.metainfo_charset = Some(default_charset.clone()),
                            'n' => opts.metainfo_charset = None,
                            other => anyhow::bail!("error: unknown switch `{}'\n{}", other, MAILINFO_USAGE),
                        }
                    }
                }
            }
        }
    }

    let [msg, patch] = files.as_slice() else { anyhow::bail!("{}", MAILINFO_USAGE) };
    opts.msg_file = msg.clone();
    opts.patch_file = patch.clone();
    Ok(opts)
}

/// Maps a `--quoted-cr` / `mailinfo.quotedCr` value to its action, failing like git on an
/// unknown one.
// Used by `parse_mailinfo_args`.
fn parse_quoted_cr_action(action: &str) -> anyhow::Result<QuotedCrAction> {
    match action {
        "nowarn" => Ok(QuotedCrAction::NoWarn),
        "warn" => Ok(QuotedCrAction::Warn),
        "strip" => Ok(QuotedCrAction::Strip),
        other => anyhow::bail!("fatal: bad action '{}' for '--quoted-cr'", other),
    }
}

/// Port of git's `mailinfo()`: reads one email and separates it into the commit message,
/// the patch, and the `Author:` / `Email:` / `Subject:` / `Date:` summary. Handles folded
/// and RFC 2047-encoded headers, in-body headers, scissors lines, MIME multipart bodies,
/// base64 and quoted-printable transfer encodings, `format=flowed`, and charset conversion
/// to UTF-8. `failed` is set when the mail was empty or could not be decoded cleanly.
// Used by `mailinfo`.
pub fn extract_mail_info(input: Vec<u8>, opts: &MailinfoOptions) -> MailinfoOutput {
    let mut st = MailinfoState { input, header_stage: true, ..Default::default() };

    st.pos = st.input.iter().position(|&b| !is_space(b)).unwrap_or(st.input.len());
    if st.pos >= st.input.len() {
        st.eof = true;
        eprintln!("error: empty patch: '{}'", opts.patch_file);
        return MailinfoOutput { failed: true, ..Default::default() };
    }

    let mut line = Vec::new();
    while read_one_header_line(&mut st, &mut line) {
        check_header(&mut st, opts, &line, true, true);
    }
    handle_body(&mut st, opts, &mut line);
    let info = handle_info(&mut st, opts);

    MailinfoOutput { message: st.log_message, patch: st.patch, info, failed: st.input_error }
}

/// C's `strbuf_getwholeline` on the mail: replaces `line` with the next line including its
/// LF (the last line may lack one) and returns true. At end of input it returns false, and,
/// exactly like the C code, leaves `line` untouched when the end had already been hit by an
/// earlier read (stdio's `feof`), but empties it otherwise.
// Used by `mailinfo` (internal to this file's helpers).
fn getwholeline(st: &mut MailinfoState, line: &mut Vec<u8>) -> bool {
    if st.eof {
        return false;
    }
    line.clear();
    let rest = &st.input[st.pos..];
    match rest.iter().position(|&b| b == b'\n') {
        Some(nl) => {
            line.extend_from_slice(&rest[..=nl]);
            st.pos += nl + 1;
        }
        None => {
            line.extend_from_slice(rest);
            st.pos = st.input.len();
            st.eof = true;
        }
    }
    !line.is_empty()
}

/// C's `strbuf_getline_lf`: like `getwholeline`, without the LF.
// Used by `mailinfo` (internal to this file's helpers).
fn getline_lf(st: &mut MailinfoState, line: &mut Vec<u8>) -> bool {
    if !getwholeline(st, line) {
        return false;
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    }
    true
}

/// Port of git's `is_rfc2822_header`: an mbox `From ` line or `<field-name>:` made of
/// printable non-space characters.
// Used by `mailinfo` (internal to this file's helpers).
fn is_rfc2822_header(line: &[u8]) -> bool {
    if line.starts_with(b"From ") || line.starts_with(b">From ") {
        return true;
    }
    for &ch in line {
        if ch == b':' {
            return true;
        }
        if !((33..=57).contains(&ch) || (59..=126).contains(&ch)) {
            break;
        }
    }
    false
}

/// Port of git's `read_one_header_line`: reads one header with its folded continuation
/// lines joined by single spaces. Returns false (leaving the offending line, newline
/// re-added, in `line`) at the blank line or first non-header line that ends the headers.
// Used by `mailinfo` (internal to this file's helpers).
fn read_one_header_line(st: &mut MailinfoState, line: &mut Vec<u8>) -> bool {
    if !getline_lf(st, line) {
        return false;
    }
    while line.last().is_some_and(|&b| is_space(b)) {
        line.pop();
    }
    if line.is_empty() || !is_rfc2822_header(line) {
        line.push(b'\n');
        return false;
    }
    let mut continuation = Vec::new();
    loop {
        match st.input.get(st.pos) {
            None => {
                st.eof = true;
                break;
            }
            Some(b' ') | Some(b'\t') => {}
            Some(_) => break,
        }
        if !getline_lf(st, &mut continuation) {
            break;
        }
        continuation[0] = b' ';
        while continuation.last().is_some_and(|&b| is_space(b)) {
            continuation.pop();
        }
        line.extend_from_slice(&continuation);
    }
    true
}

/// git's `skip_header`: the value of `line` if it is the header `name` (any case), with
/// leading whitespace dropped.
// Used by `mailinfo` (internal to this file's helpers).
fn skip_header<'a>(line: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let n = name.len();
    if line.len() <= n || !line[..n].eq_ignore_ascii_case(name.as_bytes()) || line[n] != b':' {
        return None;
    }
    let rest = &line[n + 1..];
    let start = rest.iter().position(|&b| !is_space(b)).unwrap_or(rest.len());
    Some(&rest[start..])
}

/// git's `parse_header`: the RFC 2047-decoded value of header `name`, if `line` is one.
// Used by `mailinfo` (internal to this file's helpers).
fn parse_header(st: &mut MailinfoState, opts: &MailinfoOptions, line: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut value = skip_header(line, name)?.to_vec();
    decode_header(st, opts, &mut value);
    Some(value)
}

/// Port of git's `check_header`: records `From`/`Subject`/`Date` (into the mail-header slots
/// when `primary`, else the in-body ones; an already filled slot is only replaced with
/// `overwrite`) and applies `Content-Type`, `Content-Transfer-Encoding` and `Message-ID`.
/// Returns whether the line was one of those headers.
// Used by `mailinfo` (internal to this file's helpers).
fn check_header(st: &mut MailinfoState, opts: &MailinfoOptions, line: &[u8], primary: bool, overwrite: bool) -> bool {
    for (i, name) in MAIL_HEADERS.iter().enumerate() {
        let filled = if primary { st.p_hdr[i].is_some() } else { st.s_hdr[i].is_some() };
        if !filled || overwrite {
            if let Some(value) = parse_header(st, opts, line, name) {
                if primary {
                    st.p_hdr[i] = Some(value);
                } else {
                    st.s_hdr[i] = Some(value);
                }
                return true;
            }
        }
    }
    if let Some(value) = parse_header(st, opts, line, "Content-Type") {
        handle_content_type(st, &value);
        return true;
    }
    if let Some(value) = parse_header(st, opts, line, "Content-Transfer-Encoding") {
        st.transfer_encoding = if find_bytes_ignore_case(&value, b"base64").is_some() {
            MailTransferEncoding::Base64
        } else if find_bytes_ignore_case(&value, b"quoted-printable").is_some() {
            MailTransferEncoding::QuotedPrintable
        } else {
            MailTransferEncoding::DontCare
        };
        return true;
    }
    if let Some(value) = parse_header(st, opts, line, "Message-ID") {
        if opts.add_message_id {
            st.message_id = Some(value);
        }
        return true;
    }
    false
}

/// git's `slurp_attr`: the value of a `name=` attribute in a header (quoted, or up to the
/// next `;` or blank).
// Used by `mailinfo` (internal to this file's helpers).
fn slurp_attr(line: &[u8], name: &[u8]) -> Option<Vec<u8>> {
    let at = find_bytes_ignore_case(line, name)? + name.len();
    let rest = &line[at..];
    let (rest, ends): (&[u8], &[u8]) = if rest.first() == Some(&b'"') { (&rest[1..], b"\"") } else { (rest, b"; \t") };
    let len = rest.iter().position(|b| ends.contains(b)).unwrap_or(rest.len());
    Some(rest[..len].to_vec())
}

/// Port of git's `handle_content_type`: notes `format=flowed` / `delsp=yes`, pushes a
/// multipart `boundary=` onto the boundary stack, and remembers the `charset=`.
// Used by `mailinfo` (internal to this file's helpers).
fn handle_content_type(st: &mut MailinfoState, line: &[u8]) {
    let has_value = |name: &[u8], value: &[u8]| slurp_attr(line, name).is_some_and(|v| v.eq_ignore_ascii_case(value));
    st.format_flowed = has_value(b"format=", b"flowed");
    st.delsp = has_value(b"delsp=", b"yes");

    if let Some(boundary) = slurp_attr(line, b"boundary=") {
        if st.boundaries.len() >= MAX_BOUNDARIES {
            eprintln!("error: Too many boundaries to handle");
            st.input_error = true;
            return;
        }
        let mut marker = b"--".to_vec();
        marker.extend_from_slice(&boundary);
        st.boundaries.push(marker);
    }
    st.charset = slurp_attr(line, b"charset=").unwrap_or_default();
}

/// Port of git's `decode_q_segment`: quoted-printable (`=XX`, and `_` for a space in RFC
/// 2047 headers); a soft line break (`=` at the end of the line) drops the newline.
// Used by `mailinfo` (internal to this file's helpers).
fn decode_q_segment(segment: &[u8], rfc2047: bool) -> Vec<u8> {
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(segment.len());
    let mut i = 0;
    while i < segment.len() {
        let c = segment[i];
        i += 1;
        if c == b'=' {
            match segment.get(i) {
                None | Some(b'\n') => break,
                Some(&d) => match (hex(d), segment.get(i + 1).and_then(|&e| hex(e))) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi * 16 + lo) as u8);
                        i += 2;
                    }
                    _ => out.push(c),
                },
            }
            continue;
        }
        out.push(if rfc2047 && c == b'_' { b' ' } else { c });
    }
    out
}

/// Port of git's `decode_b_segment`: base64 that skips anything outside the alphabet
/// (padding, line breaks, garbage).
// Used by `mailinfo` (internal to this file's helpers).
fn decode_b_segment(segment: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(segment.len());
    let (mut pos, mut acc) = (0, 0u32);
    for &c in segment {
        let v = match c {
            b'+' => 62,
            b'/' => 63,
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a') as u32 + 26,
            b'0'..=b'9' => (c - b'0') as u32 + 52,
            _ => continue,
        };
        match pos {
            0 => acc = v << 2,
            1 => {
                out.push((acc | (v >> 4)) as u8);
                acc = (v & 15) << 4;
            }
            2 => {
                out.push((acc | (v >> 2)) as u8);
                acc = (v & 3) << 6;
            }
            _ => out.push((acc | v) as u8),
        }
        pos = (pos + 1) % 4;
    }
    out
}

/// Says whether two charset names mean the same encoding (git's `same_encoding`: any case,
/// and `utf8` equals `utf-8`).
// Used by `mailinfo` (internal to this file's helpers).
fn same_encoding(a: &str, b: &str) -> bool {
    let is_utf8 = |s: &str| s.eq_ignore_ascii_case("utf-8") || s.eq_ignore_ascii_case("utf8");
    (is_utf8(a) && is_utf8(b)) || a.eq_ignore_ascii_case(b)
}

/// The Unicode characters of Windows-1252 bytes 0x80..0x9F (the rest equals ISO-8859-1).
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}', '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}',
    '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}', '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}',
    '\u{17E}', '\u{178}',
];

/// The eight positions where ISO-8859-15 differs from ISO-8859-1.
const LATIN9_DIFF: [(u8, char); 8] =
    [(0xA4, '\u{20AC}'), (0xA6, '\u{160}'), (0xA8, '\u{161}'), (0xB4, '\u{17D}'), (0xB8, '\u{17E}'), (0xBC, '\u{152}'), (0xBD, '\u{153}'), (0xBE, '\u{178}')];

/// Maps a charset name to the single-byte table used for it: 0 UTF-8, 1 US-ASCII,
/// 2 ISO-8859-1, 3 Windows-1252, 4 ISO-8859-15; `None` for charsets rgit can't convert.
// Used by `mailinfo` (internal to this file's helpers).
fn charset_kind(name: &str) -> Option<u8> {
    match name.to_ascii_lowercase().replace('_', "-").as_str() {
        "utf-8" | "utf8" => Some(0),
        "us-ascii" | "ascii" | "ansi-x3.4-1968" | "646" => Some(1),
        "iso-8859-1" | "iso8859-1" | "latin1" | "latin-1" | "l1" => Some(2),
        "windows-1252" | "cp1252" | "cp-1252" => Some(3),
        "iso-8859-15" | "iso8859-15" | "latin9" | "latin-9" => Some(4),
        _ => None,
    }
}

/// Re-codes `bytes` from one charset to another, like git's `reencode_string`, for the
/// charsets rgit knows (UTF-8, US-ASCII, ISO-8859-1, ISO-8859-15, Windows-1252). `None`
/// when a charset is unknown, the input is invalid, or a character has no place in `to`.
// Used by `mailinfo` (internal to this file's helpers).
fn reencode_bytes(bytes: &[u8], to: &str, from: &str) -> Option<Vec<u8>> {
    let (from, to) = (charset_kind(from)?, charset_kind(to)?);
    let decode_byte = |b: u8| -> Option<char> {
        Some(match from {
            1 if b >= 0x80 => return None,
            3 if (0x80..0xA0).contains(&b) => CP1252_HIGH[(b - 0x80) as usize],
            4 => LATIN9_DIFF.iter().find(|(byte, _)| *byte == b).map(|(_, c)| *c).unwrap_or(b as char),
            _ => b as char,
        })
    };
    let text: String = if from == 0 { String::from_utf8(bytes.to_vec()).ok()? } else { bytes.iter().map(|&b| decode_byte(b)).collect::<Option<String>>()? };
    if to == 0 {
        return Some(text.into_bytes());
    }
    text.chars()
        .map(|c| {
            let code = c as u32;
            match to {
                1 => (code < 0x80).then_some(code as u8),
                3 => CP1252_HIGH.iter().position(|h| *h == c).map(|i| 0x80 + i as u8).or((code < 0x80 || (0xA0..0x100).contains(&code)).then_some(code as u8)),
                4 => LATIN9_DIFF.iter().find(|(_, ch)| *ch == c).map(|(b, _)| *b).or((code < 0x100 && !LATIN9_DIFF.iter().any(|(b, _)| *b as u32 == code)).then_some(code as u8)),
                _ => (code < 0x100).then_some(code as u8),
            }
        })
        .collect()
}

/// Port of git's `convert_to_utf8`: re-codes `line` from `charset` to the metadata
/// encoding unless re-coding is off (`-n`), the charset is unknown, or both are the same.
/// Returns false (after flagging the input as bad) when the conversion isn't possible.
// Used by `mailinfo` (internal to this file's helpers).
fn convert_to_utf8(st: &mut MailinfoState, opts: &MailinfoOptions, line: &mut Vec<u8>, charset: &[u8]) -> bool {
    let Some(target) = opts.metainfo_charset.as_deref() else { return true };
    if charset.is_empty() {
        return true;
    }
    let charset = String::from_utf8_lossy(charset).into_owned();
    if same_encoding(target, &charset) {
        return true;
    }
    match reencode_bytes(line, target, &charset) {
        Some(out) => {
            *line = out;
            true
        }
        None => {
            st.input_error = true;
            eprintln!("error: cannot convert from {} to {}", charset, target);
            false
        }
    }
}

/// Port of git's `decode_header`: replaces RFC 2047 encoded words (`=?charset?B|Q?...?=`)
/// with their decoded, re-coded text, dropping whitespace between adjacent encoded words.
/// A malformed encoded word leaves the header untouched and flags the input as bad.
// Used by `mailinfo` (internal to this file's helpers).
fn decode_header(st: &mut MailinfoState, opts: &MailinfoOptions, it: &mut Vec<u8>) {
    let mut out = Vec::with_capacity(it.len());
    let mut pos = 0;
    while pos <= it.len() {
        let Some(rel) = find_bytes(&it[pos..], b"=?") else { break };
        let mut ep = pos + rel;
        if pos != ep && (!it[pos..ep].iter().all(|&b| is_space(b)) || pos == 0) {
            out.extend_from_slice(&it[pos..ep]);
        }
        ep += 2;

        let decoded = (|| {
            if ep >= it.len() {
                return None;
            }
            let cp = ep + it[ep..].iter().position(|&b| b == b'?')?;
            if cp + 3 > it.len() || it[cp + 2] != b'?' {
                return None;
            }
            let end = cp + 3 + find_bytes(&it[cp + 3..], b"?=")?;
            let piece = &it[cp + 3..end];
            let text = match it[cp + 1].to_ascii_lowercase() {
                b'b' => decode_b_segment(piece),
                b'q' => decode_q_segment(piece, true),
                _ => return None,
            };
            Some((text, it[ep..cp].to_vec(), end + 2))
        })();
        let Some((mut text, charset, next)) = decoded else {
            st.input_error = true;
            return;
        };
        if !convert_to_utf8(st, opts, &mut text, &charset) {
            return;
        }
        out.extend_from_slice(&text);
        pos = next;
    }
    out.extend_from_slice(&it[pos.min(it.len())..]);
    *it = out;
}

/// git's `is_multipart_boundary`: the line starts with the innermost boundary marker.
// Used by `mailinfo` (internal to this file's helpers).
fn is_multipart_boundary(st: &MailinfoState, line: &[u8]) -> bool {
    st.boundaries.last().is_some_and(|top| line.starts_with(top))
}

/// git's `find_boundary`: skips lines up to the next innermost boundary marker (left in
/// `line` without its newline); false when the input ends first.
// Used by `mailinfo` (internal to this file's helpers).
fn find_boundary(st: &mut MailinfoState, line: &mut Vec<u8>) -> bool {
    while getline_lf(st, line) {
        if is_multipart_boundary(st, line) {
            return true;
        }
    }
    false
}

/// Port of git's `handle_boundary`: at an end marker (`--boundary--`) pops the boundary and
/// moves on to the next one; at a part's start reads that part's headers and its first body
/// line. Returns false when there is nothing more to read.
// Used by `mailinfo` (internal to this file's helpers).
fn handle_boundary(st: &mut MailinfoState, opts: &MailinfoOptions, line: &mut Vec<u8>) -> bool {
    loop {
        let Some(top_len) = st.boundaries.last().map(|top| top.len()) else { break };
        if line.len() >= top_len + 2 && &line[top_len..top_len + 2] == b"--" {
            st.boundaries.pop();
            handle_filter(st, opts, &mut b"\n".to_vec());
            if st.input_error || !find_boundary(st, line) {
                return false;
            }
            continue;
        }
        break;
    }

    st.transfer_encoding = MailTransferEncoding::DontCare;
    st.charset.clear();
    while read_one_header_line(st, line) {
        let header = line.clone();
        check_header(st, opts, &header, true, false);
    }
    getwholeline(st, line)
}

/// git's `summarize_quoted_cr`: warns once per MIME part when a decoded line ended in CRLF.
// Used by `mailinfo` (internal to this file's helpers).
fn summarize_quoted_cr(st: &MailinfoState, opts: &MailinfoOptions) {
    if st.have_quoted_cr && opts.quoted_cr == QuotedCrAction::Warn {
        eprintln!("warning: quoted CRLF detected");
    }
}

/// Port of git's `handle_body`: walks the body line by line, switching MIME parts at
/// boundaries, undoing base64 / quoted-printable (decoded text is re-split into lines),
/// and feeding every line to the message/patch filter.
// Used by `mailinfo` (internal to this file's helpers).
fn handle_body(st: &mut MailinfoState, opts: &MailinfoOptions, line: &mut Vec<u8>) {
    let mut prev: Vec<u8> = Vec::new();

    if !st.boundaries.is_empty() && !find_boundary(st, line) {
        return;
    }

    loop {
        if is_multipart_boundary(st, line) {
            if !prev.is_empty() {
                handle_filter(st, opts, &mut prev);
                prev.clear();
            }
            summarize_quoted_cr(st, opts);
            st.have_quoted_cr = false;
            if !handle_boundary(st, opts, line) {
                return;
            }
        }

        match st.transfer_encoding {
            MailTransferEncoding::DontCare => handle_filter_flowed(st, opts, line, &mut prev),
            encoding => {
                let decoded = if encoding == MailTransferEncoding::Base64 { decode_b_segment(line) } else { decode_q_segment(line, false) };
                let mut full = std::mem::take(&mut prev);
                full.extend_from_slice(&decoded);
                let pieces: Vec<&[u8]> = full.split_inclusive(|&b| b == b'\n').collect();
                for (i, piece) in pieces.iter().enumerate() {
                    if i + 1 == pieces.len() && piece.last() != Some(&b'\n') {
                        prev.extend_from_slice(piece);
                        break;
                    }
                    handle_filter_flowed(st, opts, &mut piece.to_vec(), &mut prev);
                }
            }
        }

        if st.input_error {
            break;
        }
        if !getwholeline(st, line) {
            break;
        }
    }

    if !prev.is_empty() {
        handle_filter(st, opts, &mut prev);
    }
    summarize_quoted_cr(st, opts);
    flush_inbody_header_accum(st, opts);
}

/// Port of git's `handle_filter_flowed`: passes a body line on, first noting (or stripping)
/// a CRLF ending, or, for `format=flowed` text, undoing space-stuffing and joining
/// soft-wrapped lines (kept in `prev` until the paragraph's last line arrives).
// Used by `mailinfo` (internal to this file's helpers).
fn handle_filter_flowed(st: &mut MailinfoState, opts: &MailinfoOptions, line: &mut Vec<u8>, prev: &mut Vec<u8>) {
    let mut len = line.len();

    if !st.format_flowed {
        if line.ends_with(b"\r\n") {
            st.have_quoted_cr = true;
            if opts.quoted_cr == QuotedCrAction::Strip {
                line.truncate(len - 2);
                line.push(b'\n');
            }
        }
        handle_filter(st, opts, line);
        return;
    }

    if len > 0 && line[len - 1] == b'\n' {
        len -= 1;
        if len > 0 && line[len - 1] == b'\r' {
            len -= 1;
        }
    }

    // The signature separator stays as it is.
    if line.starts_with(b"-- ") && len == 3 {
        if !prev.is_empty() {
            handle_filter(st, opts, prev);
            prev.clear();
        }
        handle_filter(st, opts, line);
        return;
    }

    if len > 0 && line[0] == b' ' {
        line.remove(0);
        len -= 1;
    }

    if len > 0 && line[len - 1] == b' ' {
        prev.extend_from_slice(&line[..len - usize::from(st.delsp)]);
        return;
    }

    let mut full = std::mem::take(prev);
    full.extend_from_slice(line);
    handle_filter(st, opts, &mut full);
}

/// git's `handle_filter`: lines belong to the commit message until the patch starts, then
/// to the patch.
// Used by `mailinfo` (internal to this file's helpers).
fn handle_filter(st: &mut MailinfoState, opts: &MailinfoOptions, line: &mut Vec<u8>) {
    if !st.in_patch {
        if !handle_commit_msg(st, opts, line) {
            return;
        }
        st.in_patch = true;
    }
    st.patch.extend_from_slice(line);
    st.patch_lines += 1;
}

/// Port of git's `handle_commit_msg`: skips blank lines before the message, collects
/// in-body headers, re-codes the line, restarts the message at a scissors line, and
/// returns true at the line that begins the patch (adding `Message-ID:` first with `-m`).
// Used by `mailinfo` (internal to this file's helpers).
fn handle_commit_msg(st: &mut MailinfoState, opts: &MailinfoOptions, line: &mut Vec<u8>) -> bool {
    if st.header_stage && (line.is_empty() || line.as_slice() == b"\n") {
        if !st.inbody_header_accum.is_empty() {
            flush_inbody_header_accum(st, opts);
            st.header_stage = false;
        }
        return false;
    }

    if opts.use_inbody_headers && st.header_stage {
        st.header_stage = check_inbody_header(st, opts, line);
        if st.header_stage {
            return false;
        }
    } else {
        st.header_stage = false;
    }

    let charset = st.charset.clone();
    if !convert_to_utf8(st, opts, line, &charset) {
        return false;
    }

    if opts.use_scissors && is_scissors_line(line) {
        st.log_message.clear();
        st.header_stage = true;
        st.s_hdr = Default::default();
        return false;
    }

    if is_patch_break(line) {
        if let Some(id) = &st.message_id {
            st.log_message.extend_from_slice(b"Message-ID: ");
            st.log_message.extend_from_slice(id);
            st.log_message.push(b'\n');
        }
        return true;
    }

    st.log_message.extend_from_slice(line);
    false
}

/// git's `flush_inbody_header_accum`: applies the in-body header collected so far.
// Used by `mailinfo` (internal to this file's helpers).
fn flush_inbody_header_accum(st: &mut MailinfoState, opts: &MailinfoOptions) {
    if st.inbody_header_accum.is_empty() {
        return;
    }
    let header = std::mem::take(&mut st.inbody_header_accum);
    check_header(st, opts, &header, false, false);
}

/// Port of git's `check_inbody_header`: says whether a line at the top of the body is an
/// in-body header (`From:`, `Subject:`, `Date:`, their continuation lines, a `>From `
/// format-patch separator, or a `[PATCH] ...` subject), collecting it if so.
// Used by `mailinfo` (internal to this file's helpers).
fn check_inbody_header(st: &mut MailinfoState, opts: &MailinfoOptions, line: &[u8]) -> bool {
    if !st.inbody_header_accum.is_empty() && matches!(line.first(), Some(b' ') | Some(b'\t')) {
        if opts.use_scissors && is_scissors_line(line) {
            flush_inbody_header_accum(st, opts);
            return false;
        }
        if st.inbody_header_accum.last() == Some(&b'\n') {
            st.inbody_header_accum.pop();
        }
        st.inbody_header_accum.extend_from_slice(line);
        return true;
    }

    flush_inbody_header_accum(st, opts);

    if line.starts_with(b">From") && line.get(5).is_some_and(|&b| is_space(b)) {
        return is_format_patch_separator(&line[1..]);
    }
    if line.starts_with(b"[PATCH]") && line.get(7).is_some_and(|&b| is_space(b)) {
        st.s_hdr[1] = Some(line.to_vec());
        return true;
    }
    let is_header = MAIL_HEADERS.iter().enumerate().any(|(i, name)| st.s_hdr[i].is_none() && skip_header(line, name).is_some());
    if is_header {
        st.inbody_header_accum.extend_from_slice(line);
    }
    is_header
}

/// git's `is_format_patch_separator`: `From <40 hex> Mon Sep 17 00:00:00 2001`.
// Used by `mailinfo` (internal to this file's helpers).
fn is_format_patch_separator(line: &[u8]) -> bool {
    const SAMPLE: &[u8] = b"From 0000000000000000000000000000000000000000 Mon Sep 17 00:00:00 2001\n";
    line.len() == SAMPLE.len()
        && line.starts_with(b"From ")
        && line[5..45].iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && line[45..] == SAMPLE[45..]
}

/// Port of git's `patchbreak`: `diff -`, `Index: `, `--- <file>`, or a `---` line with
/// nothing but whitespace after it.
// Used by `mailinfo` (internal to this file's helpers).
fn is_patch_break(line: &[u8]) -> bool {
    if line.starts_with(b"diff -") || line.starts_with(b"Index: ") {
        return true;
    }
    if line.len() < 4 || !line.starts_with(b"---") {
        return false;
    }
    if line[3] == b' ' && !line.get(4).is_some_and(|&b| is_space(b)) {
        return true;
    }
    for &c in &line[3..] {
        if c == b'\n' {
            return true;
        }
        if !is_space(c) {
            break;
        }
    }
    false
}

/// Port of git's `is_scissors_line`: a line such as `-- >8 --` whose dashes and scissors
/// (`>8`, `8<`, `>%`, `%<`) make up enough of its visible width.
// Used by `mailinfo` (internal to this file's helpers).
fn is_scissors_line(line: &[u8]) -> bool {
    let (mut scissors, mut gap, mut perforation) = (0usize, 0usize, 0usize);
    let (mut first_nonblank, mut last_nonblank) = (None, None);
    let mut in_perforation = false;

    let mut i = 0;
    while i < line.len() {
        let c = line[i];
        if is_space(c) {
            if in_perforation {
                perforation += 1;
                gap += 1;
            }
            i += 1;
            continue;
        }
        last_nonblank = Some(i);
        if first_nonblank.is_none() {
            first_nonblank = Some(i);
        }
        if c == b'-' {
            in_perforation = true;
            perforation += 1;
            i += 1;
            continue;
        }
        let pair = &line[i..(i + 2).min(line.len())];
        if pair == b">8" || pair == b"8<" || pair == b">%" || pair == b"%<" {
            in_perforation = true;
            perforation += 2;
            scissors += 2;
            i += 2;
            continue;
        }
        in_perforation = false;
        i += 1;
    }

    let visible = match (first_nonblank, last_nonblank) {
        (Some(first), Some(last)) => last - first + 1,
        _ => 0,
    };
    scissors > 0 && visible >= 8 && visible < perforation * 3 && gap * 2 < perforation
}

/// git's `cleanup_space`: every run of whitespace becomes one space.
// Used by `mailinfo` (internal to this file's helpers).
fn cleanup_space(buf: &mut Vec<u8>) {
    let mut out = Vec::with_capacity(buf.len());
    let mut in_space = false;
    for &b in buf.iter() {
        if is_space(b) {
            if !in_space {
                out.push(b' ');
            }
            in_space = true;
        } else {
            out.push(b);
            in_space = false;
        }
    }
    *buf = out;
}

/// Port of git's `cleanup_subject`: strips leading `Re:`, blanks, colons and bracketed
/// prefixes such as `[PATCH 2/3]`; with `-b` only brackets that mention `PATCH` go.
// Used by `mailinfo` (internal to this file's helpers).
fn cleanup_subject(subject: &mut Vec<u8>, keep_non_patch_brackets: bool) {
    let mut at = 0;
    while at < subject.len() {
        match subject[at] {
            b'r' | b'R' => {
                if subject.len() > at + 3 && subject[at + 1].eq_ignore_ascii_case(&b'e') && subject[at + 2] == b':' {
                    subject.drain(at..at + 3);
                    continue;
                }
                break;
            }
            b' ' | b'\t' | b':' => {
                subject.remove(at);
                continue;
            }
            b'[' => {
                let Some(close) = subject[at..].iter().position(|&b| b == b']') else { break };
                let remove = close + 1;
                if !keep_non_patch_brackets || (remove >= 7 && find_bytes(&subject[at..at + remove], b"PATCH").is_some()) {
                    subject.drain(at..at + remove);
                } else {
                    at += remove;
                    if subject.get(at).is_some_and(|&b| is_space(b)) {
                        at += 1;
                    }
                }
                continue;
            }
            _ => break,
        }
    }
    trim_in_place(subject);
}

/// git's `unquote_quoted_pair`: removes the quotes of `"..."` strings and the backslash
/// escapes inside them and inside `(...)` comments.
// Used by `mailinfo` (internal to this file's helpers).
fn unquote_quoted_pair(line: &mut Vec<u8>) {
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        let c = line[i];
        i += 1;
        match c {
            b'"' => {
                let mut literal = false;
                while i < line.len() {
                    let c = line[i];
                    i += 1;
                    if literal {
                        literal = false;
                    } else if c == b'\\' {
                        literal = true;
                        continue;
                    } else if c == b'"' {
                        break;
                    }
                    out.push(c);
                }
            }
            b'(' => {
                out.push(b'(');
                let (mut literal, mut depth) = (false, 1);
                while i < line.len() {
                    let c = line[i];
                    i += 1;
                    if literal {
                        literal = false;
                    } else if c == b'\\' {
                        literal = true;
                        continue;
                    } else if c == b'(' {
                        depth += 1;
                    } else if c == b')' {
                        depth -= 1;
                        if depth == 0 {
                            out.push(c);
                            break;
                        }
                    }
                    out.push(c);
                }
            }
            _ => out.push(c),
        }
    }
    *line = out;
}

/// git's `get_sane_name`: the name, unless it is empty, too long or looks like an
/// address, in which case the email stands in for it.
// Used by `mailinfo` (internal to this file's helpers).
fn sane_name(name: &[u8], email: &[u8]) -> Vec<u8> {
    if name.is_empty() || name.len() > 60 || name.iter().any(|b| matches!(b, b'@' | b'<' | b'>')) {
        email.to_vec()
    } else {
        name.to_vec()
    }
}

/// Port of git's `handle_from`: splits a `From` value into email (the word around `@`,
/// with or without `<>`) and name (what is left, minus quotes and a surrounding `()`),
/// accepting `Name <email>`, `email (Name)` and `Name <not-an-address>`.
// Used by `mailinfo` (internal to this file's helpers).
fn handle_from(st: &mut MailinfoState, from: &[u8]) {
    let mut f = from.to_vec();
    unquote_quoted_pair(&mut f);

    let Some(mut at) = f.iter().position(|&b| b == b'@') else {
        // git's `parse_bogus_from`: "John Doe <johndoe>".
        if !st.email.is_empty() {
            return;
        }
        let Some(bra) = from.iter().position(|&b| b == b'<') else { return };
        let Some(ket) = from[bra..].iter().position(|&b| b == b'>') else { return };
        st.email = from[bra + 1..bra + ket].to_vec();
        let mut name = from[..bra].to_vec();
        trim_in_place(&mut name);
        st.name = sane_name(&name, &st.email);
        return;
    };

    if !st.email.is_empty() && f[at + 1..].contains(&b'@') {
        return;
    }

    while at > 0 {
        let c = f[at - 1];
        if is_space(c) {
            break;
        }
        if c == b'<' {
            f[at - 1] = b' ';
            break;
        }
        at -= 1;
    }
    let el = f[at..].iter().position(|b| matches!(b, b' ' | b'\n' | b'\t' | b'\r' | 0x0b | 0x0c | b'>')).unwrap_or(f.len() - at);
    st.email = f[at..at + el].to_vec();
    let end = (at + el + usize::from(at + el < f.len())).min(f.len());
    f.drain(at..end);

    cleanup_space(&mut f);
    trim_in_place(&mut f);
    if f.len() >= 2 && f[0] == b'(' && f[f.len() - 1] == b')' {
        f.remove(0);
        f.pop();
    } else if f.as_slice() == b"(" {
        f.clear();
    }
    st.name = sane_name(&f, &st.email);
}

/// Port of git's `handle_info`: renders the summary block. In-body headers win over mail
/// headers once a patch was found; the subject is cleaned up unless `-k`.
// Used by `mailinfo` (internal to this file's helpers).
fn handle_info(st: &mut MailinfoState, opts: &MailinfoOptions) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, name) in MAIL_HEADERS.iter().enumerate() {
        let header = if st.patch_lines > 0 && st.s_hdr[i].is_some() { st.s_hdr[i].clone() } else { st.p_hdr[i].clone() };
        let Some(mut hdr) = header else { continue };

        if hdr.contains(&0) {
            eprintln!("error: a NUL byte in '{}' is not allowed.", name);
            st.input_error = true;
        }

        match *name {
            "Subject" => {
                if !opts.keep_subject {
                    cleanup_subject(&mut hdr, opts.keep_non_patch_brackets);
                    cleanup_space(&mut hdr);
                }
                for part in until_nul(&hdr).split(|&b| b == b'\n') {
                    out.extend_from_slice(b"Subject: ");
                    out.extend_from_slice(part);
                    out.push(b'\n');
                }
            }
            "From" => {
                cleanup_space(&mut hdr);
                handle_from(st, &hdr);
                out.extend_from_slice(b"Author: ");
                out.extend_from_slice(until_nul(&st.name));
                out.extend_from_slice(b"\nEmail: ");
                out.extend_from_slice(until_nul(&st.email));
                out.push(b'\n');
            }
            _ => {
                cleanup_space(&mut hdr);
                out.extend_from_slice(name.as_bytes());
                out.extend_from_slice(b": ");
                out.extend_from_slice(until_nul(&hdr));
                out.push(b'\n');
            }
        }
    }
    out.push(b'\n');
    out
}

