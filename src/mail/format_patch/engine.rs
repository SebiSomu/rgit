use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use crate::core::commit::collect_ancestors;
use crate::core::types::ParsedCommit;
use crate::core::commit::parse_commit_object;
use crate::core::rev_parse::resolve_commit_rev;
use crate::core::date::format_rfc2822_date;

/// Selects the commits `format-patch` turns into patches, oldest first, following git:
/// `A..B` takes what `B` has and `A` lacks, a single `<since>` means `<since>..HEAD`, `--root`
/// or `-<n>` treat a single revision as the tip (default `HEAD`), and `-<n>` keeps only the `n`
/// newest. Commits are walked newest-first by committer date, and merge commits are skipped.
// Used by `format-patch`.
pub fn select_patch_commits(revs: &[String], max_count: Option<usize>, root: bool) -> anyhow::Result<Vec<ParsedCommit>> {
    let mut tips = Vec::new();
    let mut excluded = HashSet::new();

    for rev in revs {
        if let Some((from, to)) = rev.split_once("..") {
            let from = if from.is_empty() { "HEAD" } else { from };
            let to = if to.is_empty() { "HEAD" } else { to };
            excluded.extend(collect_ancestors(&resolve_commit_rev(from)?)?);
            tips.push(resolve_commit_rev(to)?);
        } else if let Some(negated) = rev.strip_prefix('^') {
            excluded.extend(collect_ancestors(&resolve_commit_rev(negated)?)?);
        } else if root || max_count.is_some() {
            tips.push(resolve_commit_rev(rev)?);
        } else {
            excluded.extend(collect_ancestors(&resolve_commit_rev(rev)?)?);
            tips.push(resolve_commit_rev("HEAD")?);
        }
    }
    if tips.is_empty() {
        tips.push(resolve_commit_rev("HEAD")?);
    }

    let mut heap = BinaryHeap::new();
    let mut seen = HashSet::new();
    let mut sequence = 0usize;
    for tip in tips {
        if seen.insert(tip.clone()) && !excluded.contains(&tip) {
            let commit = parse_commit_object(&tip)?;
            heap.push((commit.committer.timestamp, Reverse(sequence), tip));
            sequence += 1;
        }
    }

    let mut selected = Vec::new();
    while let Some((_, _, hash)) = heap.pop() {
        let commit = parse_commit_object(&hash)?;
        for parent in &commit.parents {
            if !excluded.contains(parent) && seen.insert(parent.clone()) {
                let parent_commit = parse_commit_object(parent)?;
                heap.push((parent_commit.committer.timestamp, Reverse(sequence), parent.clone()));
                sequence += 1;
            }
        }
        if commit.parents.len() <= 1 {
            selected.push(commit);
            if max_count.is_some_and(|n| selected.len() >= n) {
                break;
            }
        }
    }

    selected.reverse();
    Ok(selected)
}

/// Builds the mail header block git emits for a patch: the mbox `From <hash>` line, `From:`
/// (RFC 2047-encoded or quoted when needed), `Date:`, the folded or encoded `Subject:` with its
/// prefix, and MIME headers when the message body is not plain ASCII.
// Used by `format-patch`.
pub fn format_mail_headers(commit: &ParsedCommit, subject_prefix: &str, subject: &str, body: &str) -> String {
    let mut out = format!("From {} Mon Sep 17 00:00:00 2001\n", commit.hash);

    out.push_str("From: ");
    let name = &commit.author.name;
    let mut max_length = 78;
    if needs_rfc2047(name) {
        add_rfc2047(&mut out, name, true);
        max_length = 76;
    } else if name.chars().any(|c| "()<>@,;:\\\".[]".contains(c)) {
        let quoted = format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""));
        add_wrapped_text(&mut out, &quoted, -6, 1, 78);
    } else {
        add_wrapped_text(&mut out, name, -6, 1, 78);
    }
    if max_length < last_line_length(&out) + 2 + commit.author.email.len() + 1 {
        out.push('\n');
    }
    out.push_str(&format!(" <{}>\n", commit.author.email));
    out.push_str(&format!("Date: {}\n", format_rfc2822_date(commit.author.timestamp, &commit.author.tz)));

    out.push_str("Subject: ");
    out.push_str(subject_prefix);
    if needs_rfc2047(subject) {
        add_rfc2047(&mut out, subject, false);
    } else {
        let indent = last_line_length(&out) as i64;
        add_wrapped_text(&mut out, subject, -indent, 1, 78);
    }
    out.push('\n');

    if !body.is_ascii() || !subject.is_ascii() {
        out.push_str("MIME-Version: 1.0\nContent-Type: text/plain; charset=UTF-8\nContent-Transfer-Encoding: 8bit\n");
    }
    out
}

fn last_line_length(text: &str) -> usize {
    text.rsplit('\n').next().map(|l| l.len()).unwrap_or(0)
}

fn needs_rfc2047(text: &str) -> bool {
    !text.is_ascii() || text.contains('\n') || text.contains("=?")
}

fn add_rfc2047(out: &mut String, text: &str, address: bool) {
    const MAX_ENCODED_LENGTH: usize = 76;
    let mut line_len = last_line_length(out) + "UTF-8".len() + 5;
    out.push_str("=?UTF-8?q?");
    for ch in text.chars() {
        let mut bytes = [0u8; 4];
        let encoded_bytes = ch.encode_utf8(&mut bytes).as_bytes();
        let special = encoded_bytes.len() > 1 || is_rfc2047_special(encoded_bytes[0], address);
        let encoded_len = if special { 3 * encoded_bytes.len() } else { 1 };
        if line_len + encoded_len + 2 > MAX_ENCODED_LENGTH {
            out.push_str("?=\n =?UTF-8?q?");
            line_len = "UTF-8".len() + 5 + 1;
        }
        for &b in encoded_bytes {
            if special {
                out.push_str(&format!("={:02X}", b));
            } else {
                out.push(b as char);
            }
        }
        line_len += encoded_len;
    }
    out.push_str("?=");
}

fn is_rfc2047_special(ch: u8, address: bool) -> bool {
    if !ch.is_ascii() || matches!(ch, b' ' | b'=' | b'?' | b'_') {
        return true;
    }
    address && !(ch.is_ascii_alphanumeric() || matches!(ch, b'!' | b'*' | b'+' | b'-' | b'/'))
}

/// Port of git's `strbuf_add_wrapped_text`: appends `text` wrapped at `width` columns, with
/// a negative `indent1` meaning the current line already holds that many columns and
/// continuation lines indented by `indent2`.
// Used by `format_mail_headers` for folding long `Subject:` and `From:` headers.
pub fn add_wrapped_text(out: &mut String, text: &str, indent1: i64, indent2: i64, width: i64) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut bol = 0usize;
    let mut indent = indent1;
    let mut w = indent1;
    let mut space: Option<usize> = None;
    if indent < 0 {
        w = -indent;
        space = Some(0);
    }

    loop {
        let c = chars.get(i).copied();
        if c.is_none() || c.is_some_and(|c| c.is_ascii_whitespace()) {
            if w <= width || space.is_none() {
                let mut start = bol;
                if c.is_none() && i == start {
                    return;
                }
                match space {
                    Some(sp) => start = sp,
                    None => out.extend(std::iter::repeat_n(' ', indent.max(0) as usize)),
                }
                out.extend(&chars[start..i]);
                if c.is_none() {
                    return;
                }
                space = Some(i);
                if c == Some('\t') {
                    w |= 0x07;
                }
                w += 1;
                i += 1;
            } else {
                out.push('\n');
                let sp = space.unwrap_or(bol);
                i = sp + usize::from(chars.get(sp).is_some_and(|c| c.is_ascii_whitespace()));
                bol = i;
                space = None;
                w = indent2;
                indent = indent2;
            }
            continue;
        }
        w += 1;
        i += 1;
    }
}

/// Builds git's patch file name: `NNNN-<sanitized subject>.patch`, where runs of characters
/// other than letters, digits, `.` and `_` become a single `-`, repeated dots collapse, trailing
/// `.`/`-` are dropped, and the name before `.patch` is capped at 57 characters.
// Used by `format-patch`.
pub fn patch_file_name(number: usize, subject: &str) -> String {
    let mut sanitized = String::new();
    let mut space = 2;
    let chars: Vec<char> = subject.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            if space == 1 {
                sanitized.push('-');
            }
            space = 0;
            sanitized.push(c);
            if c == '.' {
                while chars.get(i + 1) == Some(&'.') {
                    i += 1;
                }
            }
        } else {
            space |= 1;
        }
        i += 1;
    }
    let sanitized = sanitized.trim_end_matches(['.', '-']);
    let mut name = format!("{:04}-{}", number, sanitized);
    name.truncate(64 - (".patch".len() + 1));
    format!("{}.patch", name)
}

