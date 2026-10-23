use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Context;
use base64::Engine;
use regex::Regex;
use crate::helpers::format_patch::{split_commit_message, tz_offset_seconds};
use crate::helpers::{author_identity, flatten_tree, tree_hash_of_commit, write_object};
use crate::index::IndexEntry;
use crate::objects::{CommitSignature, MailPatch};
use crate::refs;

/// Directory holding an in-progress `am` session (same layout as git: `next`, `last`,
/// `0001`... raw mails, and `orig-head`).
pub const AM_STATE_DIR: &str = ".git/rebase-apply";

/// Splits mailbox text into individual messages at mbox `From <sender> <date>` separator
/// lines (as written by `format-patch`); each message keeps its separator line, like git's
/// stored copies. Text without separators is a single message.
/// Carriage returns before line feeds are dropped, like git's default `--no-keep-cr`.
// Used by `am`.
pub fn split_mailbox(text: &str) -> Vec<String> {
    let text = text.replace("\r\n", "\n");
    let separator = Regex::new(r"^From \S+ +[A-Z][a-z]{2} [A-Z][a-z]{2} +\d{1,2} \d{2}:\d{2}:\d{2} \d{4}\s*$").expect("valid regex");
    let mut messages = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut saw_separator = false;

    for line in text.split_inclusive('\n') {
        if separator.is_match(line.trim_end_matches('\n')) {
            if saw_separator || current.iter().any(|l| !l.trim().is_empty()) {
                messages.push(current.concat());
            }
            current = vec![line];
            saw_separator = true;
            continue;
        }
        current.push(line);
    }
    if current.iter().any(|l| !l.trim().is_empty()) {
        messages.push(current.concat());
    }
    messages
}

/// Decodes one patch email: unfolds and RFC 2047-decodes the headers, takes the author from
/// `From:` and `Date:` (in-body `From:`/`Date:`/`Subject:` lines at the top of the body
/// override them), cleans the subject (dropping `Re:` and bracketed `[PATCH n/m]` prefixes
/// unless `keep_subject`), and splits the body into commit message and patch at the first
/// `---`, `diff -`, or `Index: ` line.
// Used by `am`.
pub fn parse_mail(raw: &str, keep_subject: bool) -> anyhow::Result<MailPatch> {
    let raw = match raw.split_once('\n') {
        Some((first, rest)) if first.starts_with("From ") => rest,
        _ => raw,
    };
    let (header_text, body) = match raw.find("\n\n") {
        Some(pos) => (&raw[..pos], &raw[pos + 2..]),
        None => (raw, ""),
    };
    let mut headers: BTreeMap<String, String> = BTreeMap::new();
    let mut last_key: Option<String> = None;
    for line in header_text.lines() {
        if (line.starts_with(' ') || line.starts_with('\t')) && last_key.is_some() {
            if let Some(value) = headers.get_mut(last_key.as_ref().unwrap()) {
                value.push_str(line);
            }
        } else if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            headers.entry(key.clone()).or_insert_with(|| value.trim().to_string());
            last_key = Some(key);
        }
    }

    let mut from = headers.get("from").map(|v| decode_rfc2047(v)).unwrap_or_default();
    let mut date = headers.get("date").cloned().unwrap_or_default();
    let mut subject = headers.get("subject").map(|v| decode_rfc2047(v)).unwrap_or_default();

    let mut body_lines: Vec<&str> = body.lines().collect();
    while body_lines.first().is_some_and(|l| l.trim().is_empty()) {
        body_lines.remove(0);
    }
    let mut overridden = false;
    while let Some(first) = body_lines.first() {
        if let Some(v) = first.strip_prefix("From: ") {
            from = decode_rfc2047(v.trim());
        } else if let Some(v) = first.strip_prefix("Date: ") {
            date = v.trim().to_string();
        } else if let Some(v) = first.strip_prefix("Subject: ") {
            subject = decode_rfc2047(v.trim());
        } else {
            break;
        }
        overridden = true;
        body_lines.remove(0);
    }
    if overridden {
        while body_lines.first().is_some_and(|l| l.trim().is_empty()) {
            body_lines.remove(0);
        }
    }

    let patch_start = body_lines.iter().position(|l| is_patch_break(l)).unwrap_or(body_lines.len());
    let message_body = body_lines[..patch_start].join("\n");
    let patch = body_lines[patch_start..].iter().map(|l| format!("{}\n", l)).collect::<String>();

    let subject = if keep_subject { subject.trim().to_string() } else { clean_subject(&subject) };
    let message = stripspace(&format!("{}\n\n{}", subject, message_body));
    let (name, email) = parse_address(&from);
    let (timestamp, tz) = parse_rfc2822_date(&date).unwrap_or_else(|| (now_seconds(), "+0000".to_string()));

    Ok(MailPatch {
        author: CommitSignature { name, email, timestamp, tz },
        subject: split_commit_message(&message).0,
        message,
        patch,
    })
}

fn is_patch_break(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("---") {
        if rest.trim().is_empty() {
            return true;
        }
        if rest.starts_with(' ') && !rest[1..].starts_with(char::is_whitespace) {
            return true;
        }
        return false;
    }
    line.starts_with("diff -") || line.starts_with("Index: ")
}

/// Removes leading `Re:`, whitespace, `:` and bracketed `[...]` prefixes from a mail subject,
/// as git's `mailinfo` does, so `[PATCH 2/3] Fix x` becomes `Fix x`.
// Used by `am` via `parse_mail`.
pub fn clean_subject(subject: &str) -> String {
    let mut s = subject.to_string();
    loop {
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("re:") {
            s.drain(..3);
        } else if s.starts_with([' ', '\t', ':']) {
            s.drain(..1);
        } else if s.starts_with('[') {
            match s.find(']') {
                Some(end) => {
                    s.drain(..=end);
                }
                None => break,
            }
        } else {
            break;
        }
    }
    s.trim().to_string()
}

/// Port of git's default `stripspace`: trims trailing whitespace from every line, collapses
/// runs of blank lines into one, drops leading and trailing blank lines, and ends the text
/// with a single newline (empty input stays empty).
// Used by `am` to normalise commit messages built from mail.
pub fn stripspace(text: &str) -> String {
    let mut out = String::new();
    let mut pending_blank = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            pending_blank = !out.is_empty();
            continue;
        }
        if pending_blank {
            out.push('\n');
            pending_blank = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Decodes RFC 2047 encoded words (`=?charset?Q?...?=` and `=?charset?B?...?=`) in a header,
/// dropping whitespace between adjacent encoded words. UTF-8 and ISO-8859-1/Windows-1252 are
/// decoded; other charsets are read as UTF-8.
// Used by `am` via `parse_mail`.
pub fn decode_rfc2047(value: &str) -> String {
    let word = Regex::new(r"=\?([^?\s]+)\?([QqBb])\?([^?\s]*)\?=").expect("valid regex");
    let mut out = String::new();
    let mut last_end = 0;
    let mut previous_was_word = false;

    for caps in word.captures_iter(value) {
        let whole = caps.get(0).unwrap();
        let between = &value[last_end..whole.start()];
        if !(previous_was_word && between.trim().is_empty()) {
            out.push_str(between);
        }
        let charset = caps[1].to_ascii_lowercase();
        let encoded = &caps[3];
        let bytes = if caps[2].eq_ignore_ascii_case("q") {
            decode_q(encoded)
        } else {
            base64::engine::general_purpose::STANDARD.decode(encoded).unwrap_or_default()
        };
        if charset.contains("8859") || charset.contains("1252") || charset == "latin1" {
            out.extend(bytes.iter().map(|&b| b as char));
        } else {
            out.push_str(&String::from_utf8_lossy(&bytes));
        }
        last_end = whole.end();
        previous_was_word = true;
    }
    out.push_str(&value[last_end..]);
    out
}

fn decode_q(encoded: &str) -> Vec<u8> {
    let bytes = encoded.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let hex_byte = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex_byte) {
            (b'_', _) => out.push(b' '),
            (b'=', Some(b)) => {
                out.push(b);
                i += 2;
            }
            (other, _) => out.push(other),
        }
        i += 1;
    }
    out
}

/// Splits an address header into `(name, email)`, accepting `Name <email>`,
/// `"Quoted, Name" <email>`, `email (Name)`, and a bare `email` (whose name becomes the email).
// Used by `am` via `parse_mail`.
pub fn parse_address(from: &str) -> (String, String) {
    let from = from.trim();
    if let (Some(open), Some(close)) = (from.rfind('<'), from.rfind('>')) {
        if open < close {
            let mut name = from[..open].trim().to_string();
            if name.len() >= 2 && name.starts_with('"') && name.ends_with('"') {
                name = name[1..name.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\");
            }
            let email = from[open + 1..close].trim().to_string();
            if name.is_empty() {
                name = email.clone();
            }
            return (name, email);
        }
    }
    if let (Some(open), Some(close)) = (from.find('('), from.rfind(')')) {
        if open < close {
            return (from[open + 1..close].trim().to_string(), from[..open].trim().to_string());
        }
    }
    (from.to_string(), from.to_string())
}

/// Parses an RFC 2822 date (`[Day, ]D Mon YYYY HH:MM[:SS] +ZZZZ`) into a Unix timestamp and
/// its `+HHMM` timezone.
// Used by `am` via `parse_mail`.
pub fn parse_rfc2822_date(date: &str) -> Option<(i64, String)> {
    let date = date.split_once(',').map(|(_, rest)| rest).unwrap_or(date);
    let parts: Vec<&str> = date.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    let day: i64 = parts[0].parse().ok()?;
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let month = MONTHS.iter().position(|m| parts[1].to_ascii_lowercase().starts_with(m))? as i64 + 1;
    let year: i64 = parts[2].parse().ok()?;
    let time: Vec<i64> = parts[3].split(':').map(|p| p.parse().ok()).collect::<Option<Vec<_>>>()?;
    let (h, m, s) = (time.first().copied()?, time.get(1).copied()?, time.get(2).copied().unwrap_or(0));
    let tz = parts.get(4).filter(|t| t.len() == 5 && (t.starts_with('+') || t.starts_with('-'))).map(|t| t.to_string()).unwrap_or_else(|| "+0000".to_string());
    let local = days_from_civil(year, month, day) * 86_400 + h * 3600 + m * 60 + s;
    Some((local - tz_offset_seconds(&tz), tz))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn now_seconds() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Writes a commit object with an explicit author (name, email, original date and timezone)
/// and the configured identity as committer at the current time.
// Used by `am` to record a mailed patch under its original author.
pub fn build_commit_with_author(tree_hash: &str, parent: Option<&str>, author: &CommitSignature, message: &str) -> anyhow::Result<String> {
    let mut content = format!("tree {}\n", tree_hash);
    if let Some(parent) = parent {
        content.push_str(&format!("parent {}\n", parent));
    }
    let clean = |s: &str| s.chars().filter(|c| !matches!(c, '<' | '>' | '\n' | '\r' | '\0')).collect::<String>().trim().to_string();
    content.push_str(&format!("author {} <{}> {} {}\n", clean(&author.name), clean(&author.email), author.timestamp, author.tz));
    content.push_str(&format!("committer {} {} +0000\n", author_identity()?, now_seconds()));
    content.push('\n');
    content.push_str(message);
    write_object("commit", content.as_bytes())
}

/// Moves the current branch (or a detached HEAD) to `commit_hash`, recording reflog entries.
// Used by `am` after each applied patch and by `am --abort`.
pub fn advance_head(commit_hash: &str) -> anyhow::Result<()> {
    match refs::resolve_head()? {
        refs::HeadState::Branch(branch) => refs::write_ref(&format!("refs/heads/{}", branch), commit_hash),
        refs::HeadState::Detached(_) => refs::set_head_detached(commit_hash),
    }
}

/// Returns HEAD's tree as a path map (empty for an unborn branch).
// Used by `am` for its dirty-index check, `--continue`, `--skip`, and `--abort`.
pub fn head_tree_map() -> anyhow::Result<BTreeMap<String, ([u8; 20], u32)>> {
    let mut map = BTreeMap::new();
    if let Some(head) = refs::resolve_head_commit()? {
        flatten_tree(&tree_hash_of_commit(&head)?, "", &mut map)?;
    }
    Ok(map)
}

/// Lists paths whose index entry (hash or mode) differs from HEAD's tree, in path order.
// Used by `am` for its dirty-index check and `--continue`'s "no changes" check.
pub fn index_differences(index: &[IndexEntry], head: &BTreeMap<String, ([u8; 20], u32)>) -> Vec<String> {
    let index_map: BTreeMap<&str, ([u8; 20], u32)> = index.iter().map(|e| (e.path.as_str(), (e.hash, e.mode))).collect();
    let mut paths: Vec<String> = head
        .iter()
        .filter(|(p, v)| index_map.get(p.as_str()) != Some(v))
        .map(|(p, _)| p.clone())
        .collect();
    paths.extend(index_map.keys().filter(|p| !head.contains_key(**p)).map(|p| p.to_string()));
    paths.sort();
    paths.dedup();
    paths
}

/// Resolves a file inside the `am` session directory (`next`, `last`, `orig-head`, `0001`, ...).
// Used by the `am` session-state helpers in this file.
fn am_state_file(name: &str) -> std::path::PathBuf {
    crate::repo::path(format!("{}/{}", AM_STATE_DIR, name))
}

/// Starts an `am` session: writes each raw mail as `NNNN`, plus `next`, `last`, and
/// `orig-head` (empty for an unborn branch) under `.git/rebase-apply/`.
// Used by `am`.
pub fn write_am_state(messages: &[String], orig_head: Option<&str>) -> anyhow::Result<()> {
    fs::create_dir_all(crate::repo::path(AM_STATE_DIR)).context("Failed to create .git/rebase-apply")?;
    for (i, message) in messages.iter().enumerate() {
        fs::write(am_state_file(&format!("{:04}", i + 1)), message)?;
    }
    fs::write(am_state_file("last"), format!("{}\n", messages.len()))?;
    fs::write(am_state_file("orig-head"), format!("{}\n", orig_head.unwrap_or("")))?;
    set_am_next(1)
}

/// Records which patch number the session will apply next.
// Used by `am` after each applied or skipped patch.
pub fn set_am_next(next: usize) -> anyhow::Result<()> {
    fs::write(am_state_file("next"), format!("{}\n", next)).context("Failed to write .git/rebase-apply/next")
}

/// Reads `(next, last)` from an in-progress `am` session.
// Used by `am`.
pub fn read_am_progress() -> anyhow::Result<(usize, usize)> {
    let read = |name: &str| -> anyhow::Result<usize> {
        fs::read_to_string(am_state_file(name))?.trim().parse().map_err(|_| anyhow::anyhow!("fatal: corrupt .git/rebase-apply/{}", name))
    };
    Ok((read("next")?, read("last")?))
}

/// Reads the raw mail for patch number `n` of the session.
// Used by `am`.
pub fn read_am_message(n: usize) -> anyhow::Result<String> {
    fs::read_to_string(am_state_file(&format!("{:04}", n))).with_context(|| format!("fatal: missing .git/rebase-apply/{:04}", n))
}

/// Reads the commit HEAD pointed at when the session started (`None` for an unborn branch).
// Used by `am --abort`.
pub fn read_am_orig_head() -> anyhow::Result<Option<String>> {
    let text = fs::read_to_string(am_state_file("orig-head")).context("fatal: missing .git/rebase-apply/orig-head")?;
    let hash = text.trim();
    Ok(if hash.is_empty() { None } else { Some(hash.to_string()) })
}

/// Reports whether an `am` session is in progress.
// Used by `am`.
pub fn am_in_progress() -> bool {
    crate::repo::path(AM_STATE_DIR).is_dir()
}

/// Deletes the `am` session directory.
// Used by `am` when a session finishes, and by `--abort` / `--quit`.
pub fn clear_am_state() -> anyhow::Result<()> {
    let dir = crate::repo::path(AM_STATE_DIR);
    if dir.exists() {
        fs::remove_dir_all(&dir).context("Failed to remove .git/rebase-apply")?;
    }
    Ok(())
}
