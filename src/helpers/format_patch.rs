use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashSet};
use std::io::Write;
use anyhow::Context;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use crate::helpers::bisect::collect_ancestors;
use crate::helpers::rev_parse::{peel_to_type, resolve_rev};
use crate::helpers::{flatten_tree, myers_diff, read_object};
use crate::objects::{CommitSignature, DiffOp, DiffStatEntry, FileDiff, ParsedCommit};

const CONTEXT_LINES: usize = 3;
const NO_EOL_MARK: &str = "\u{0}\\ No newline at end of file";
const BASE85_ALPHABET: &[u8; 85] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

/// Reads and parses a commit object into tree, parents, author, committer, and message.
/// Multi-line headers (such as `gpgsig`) are skipped.
// Used by `format-patch` and `am`.
pub fn parse_commit_object(hash: &str) -> anyhow::Result<ParsedCommit> {
    let (object_type, content) = read_object(hash)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: {} is not a commit", hash);
    }
    let text = String::from_utf8_lossy(&content).into_owned();
    let (header, message) = text.split_once("\n\n").unwrap_or((text.as_str(), ""));

    let mut commit = ParsedCommit { hash: hash.to_string(), message: message.to_string(), ..Default::default() };
    for line in header.lines() {
        if let Some(tree) = line.strip_prefix("tree ") {
            commit.tree = tree.to_string();
        } else if let Some(parent) = line.strip_prefix("parent ") {
            commit.parents.push(parent.to_string());
        } else if let Some(author) = line.strip_prefix("author ") {
            commit.author = parse_signature(author);
        } else if let Some(committer) = line.strip_prefix("committer ") {
            commit.committer = parse_signature(committer);
        }
    }
    Ok(commit)
}

fn parse_signature(value: &str) -> CommitSignature {
    let (name, rest) = value.split_once('<').unwrap_or((value, ""));
    let (email, tail) = rest.split_once('>').unwrap_or((rest, ""));
    let mut parts = tail.split_whitespace();
    CommitSignature {
        name: name.trim().to_string(),
        email: email.trim().to_string(),
        timestamp: parts.next().and_then(|t| t.parse().ok()).unwrap_or(0),
        tz: parts.next().unwrap_or("+0000").to_string(),
    }
}

/// Resolves any revision expression to a commit hash (peeling annotated tags).
// Used by `format-patch` commit selection.
pub fn resolve_commit_rev(rev: &str) -> anyhow::Result<String> {
    let hash = resolve_rev(rev).map_err(|_| anyhow::anyhow!("fatal: ambiguous argument '{}': unknown revision or path not in the working tree.", rev))?;
    peel_to_type(hash, "commit")
}

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

/// Splits a commit message into its subject (first paragraph, lines joined by spaces) and
/// body (everything after it, without leading blank lines or trailing whitespace).
// Used by `format-patch` and `am`.
pub fn split_commit_message(message: &str) -> (String, String) {
    let mut lines = message.lines().peekable();
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    let mut subject_lines = Vec::new();
    while let Some(line) = lines.peek() {
        if line.trim().is_empty() {
            break;
        }
        subject_lines.push(line.trim().to_string());
        lines.next();
    }
    while lines.peek().is_some_and(|l| l.trim().is_empty()) {
        lines.next();
    }
    let body: Vec<&str> = lines.collect();
    (subject_lines.join(" "), body.join("\n").trim_end().to_string())
}

/// Formats a timestamp in RFC 2822 style for mail headers, in the commit's own timezone
/// (e.g. `Sat, 3 Oct 2026 09:05:07 +0300`).
// Used by `format-patch`.
pub fn format_rfc2822_date(timestamp: i64, tz: &str) -> String {
    let offset = tz_offset_seconds(tz);
    let local = timestamp + offset;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!(
        "{}, {} {} {} {:02}:{:02}:{:02} {}",
        WEEKDAYS[days.rem_euclid(7) as usize],
        day,
        MONTHS[(month - 1) as usize],
        year,
        secs / 3600,
        (secs / 60) % 60,
        secs % 60,
        tz
    )
}

/// Converts a `+HHMM` / `-HHMM` timezone into an offset in seconds.
// Used by `format_rfc2822_date` and `am` date parsing.
pub fn tz_offset_seconds(tz: &str) -> i64 {
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let digits = tz.trim_start_matches(['+', '-']);
    if digits.len() != 4 {
        return 0;
    }
    let hours: i64 = digits[..2].parse().unwrap_or(0);
    let minutes: i64 = digits[2..].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

/// Converts days since the Unix epoch into a `(year, month, day)` civil date.
// Used by `format_rfc2822_date`.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
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

/// Produces git-format diffs for every path that differs between a commit and its first
/// parent (or the empty tree for a root commit), in path order.
// Used by `format-patch`.
pub fn diff_commit_against_parent(commit: &ParsedCommit) -> anyhow::Result<Vec<FileDiff>> {
    let mut old_tree = BTreeMap::new();
    if let Some(parent) = commit.parents.first() {
        let parent_commit = parse_commit_object(parent)?;
        flatten_tree(&parent_commit.tree, "", &mut old_tree)?;
    }
    let mut new_tree = BTreeMap::new();
    flatten_tree(&commit.tree, "", &mut new_tree)?;

    let paths: std::collections::BTreeSet<&String> = old_tree.keys().chain(new_tree.keys()).collect();
    let mut diffs = Vec::new();
    for path in paths {
        let old = old_tree.get(path).copied();
        let new = new_tree.get(path).copied();
        if old == new {
            continue;
        }
        match (old, new) {
            (Some(o), Some(n)) if (o.1 & 0o170000) != (n.1 & 0o170000) => {
                diffs.push(diff_tree_file(path, Some(o), None)?);
                diffs.push(diff_tree_file(path, None, Some(n))?);
            }
            _ => diffs.push(diff_tree_file(path, old, new)?),
        }
    }
    Ok(diffs)
}

/// Produces the `diff --git` section for one path: mode/creation/deletion headers, the
/// `index` line, and either unified hunks (3 lines of context, function-name headers, git's
/// hunk placement) or a `GIT binary patch` with zlib + base85 literals in both directions.
// Used by `format-patch` via `diff_commit_against_parent`.
pub fn diff_tree_file(path: &str, old: Option<([u8; 20], u32)>, new: Option<([u8; 20], u32)>) -> anyhow::Result<FileDiff> {
    let load = |entry: Option<([u8; 20], u32)>| -> anyhow::Result<Vec<u8>> {
        match entry {
            Some((hash, _)) => Ok(read_object(&hex::encode(hash))?.1),
            None => Ok(Vec::new()),
        }
    };
    let old_bytes = load(old)?;
    let new_bytes = load(new)?;
    let old_hex = old.map(|(h, _)| hex::encode(h)).unwrap_or_else(|| "0".repeat(40));
    let new_hex = new.map(|(h, _)| hex::encode(h)).unwrap_or_else(|| "0".repeat(40));

    let mut text = format!("diff --git a/{} b/{}\n", path, path);
    let mut summary = None;
    let mut index_mode_suffix = String::new();
    match (old, new) {
        (None, Some((_, mode))) => {
            text.push_str(&format!("new file mode {:06o}\n", mode));
            summary = Some(format!(" create mode {:06o} {}\n", mode, path));
        }
        (Some((_, mode)), None) => {
            text.push_str(&format!("deleted file mode {:06o}\n", mode));
            summary = Some(format!(" delete mode {:06o} {}\n", mode, path));
        }
        (Some((_, old_mode)), Some((_, new_mode))) if old_mode != new_mode => {
            text.push_str(&format!("old mode {:06o}\nnew mode {:06o}\n", old_mode, new_mode));
            summary = Some(format!(" mode change {:06o} => {:06o} {}\n", old_mode, new_mode, path));
        }
        (Some((_, mode)), Some(_)) => index_mode_suffix = format!(" {:06o}", mode),
        (None, None) => {}
    }

    let mut stat = DiffStatEntry { name: path.to_string(), ..Default::default() };
    if old_hex == new_hex {
        return Ok(FileDiff { text, stat, summary });
    }

    let is_binary = |b: &[u8]| b[..b.len().min(8000)].contains(&0);
    if is_binary(&old_bytes) || is_binary(&new_bytes) {
        text.push_str(&format!("index {}..{}{}\nGIT binary patch\n", old_hex, new_hex, index_mode_suffix));
        text.push_str(&encode_binary_literal(&new_bytes)?);
        text.push_str(&encode_binary_literal(&old_bytes)?);
        stat.binary_sizes = Some((old_bytes.len(), new_bytes.len()));
        return Ok(FileDiff { text, stat, summary });
    }

    text.push_str(&format!("index {}..{}{}\n", &old_hex[..7], &new_hex[..7], index_mode_suffix));
    if old_bytes.is_empty() && new_bytes.is_empty() {
        return Ok(FileDiff { text, stat, summary });
    }
    text.push_str(&format!("--- {}\n", if old.is_some() { format!("a/{}", path) } else { "/dev/null".to_string() }));
    text.push_str(&format!("+++ {}\n", if new.is_some() { format!("b/{}", path) } else { "/dev/null".to_string() }));

    let old_lines = split_diff_lines(&old_bytes);
    let new_lines = split_diff_lines(&new_bytes);
    let (hunks, added, deleted) = unified_hunks(&old_lines, &new_lines);
    text.push_str(&hunks);
    stat.added = added;
    stat.deleted = deleted;
    Ok(FileDiff { text, stat, summary })
}

/// Splits blob bytes into diff lines, marking a missing final newline the way the hunk
/// formatter expects.
// Used by `format-patch` via `diff_tree_file`, and `cherry` patch IDs.
pub fn split_diff_lines(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    if text.ends_with('\n') {
        lines.pop();
    } else if let Some(last) = lines.last_mut() {
        last.push_str(NO_EOL_MARK);
    }
    lines
}

fn display_line(line: &str) -> String {
    match line.strip_suffix(NO_EOL_MARK) {
        Some(text) => format!("{}\n\\ No newline at end of file\n", text),
        None => format!("{}\n", line),
    }
}

/// Computes git-style unified hunks between two line lists: Myers diff, then xdiff's change
/// compaction (sliding groups, aligning with the other side, indent heuristic), then hunks
/// with 3 lines of context merged when at most 6 lines apart, each headed by the nearest
/// preceding "function" line (one starting with a letter, `_`, or `$`). Returns the hunk text
/// and the added/deleted line counts.
// Used by `format-patch` via `diff_tree_file`.
pub fn unified_hunks(old: &[String], new: &[String]) -> (String, usize, usize) {
    unified_hunks_with(old, new, true)
}

/// Diffs two line lists like xdiff: Myers diff, then change compaction (sliding each changed
/// group as far down as it goes, aligning with the other side, and with `indent_heuristic`
/// picking the most readable position). Returns the changed runs as
/// `(old_start, old_count, new_start, new_count)` plus the per-line change flags of each side.
// Used by `unified_hunks_with` and `merge-tree`'s line merge.
pub fn compacted_changes(old: &[String], new: &[String], indent_heuristic: bool) -> (Vec<(usize, usize, usize, usize)>, Vec<bool>, Vec<bool>) {
    let old_refs: Vec<&str> = old.iter().map(String::as_str).collect();
    let new_refs: Vec<&str> = new.iter().map(String::as_str).collect();
    let ops = myers_diff(&old_refs, &new_refs);

    let mut rchg_old = vec![false; old.len()];
    let mut rchg_new = vec![false; new.len()];
    let (mut i, mut j) = (0, 0);
    for op in &ops {
        match op {
            DiffOp::Keep(_) => {
                i += 1;
                j += 1;
            }
            DiffOp::Delete(_) => {
                rchg_old[i] = true;
                i += 1;
            }
            DiffOp::Insert(_) => {
                rchg_new[j] = true;
                j += 1;
            }
        }
    }
    change_compact(&mut rchg_old, old, &mut rchg_new, indent_heuristic);
    change_compact(&mut rchg_new, new, &mut rchg_old, indent_heuristic);

    let mut changes: Vec<(usize, usize, usize, usize)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if rchg_old.get(i).copied().unwrap_or(false) || rchg_new.get(j).copied().unwrap_or(false) {
            let (start_i, start_j) = (i, j);
            while rchg_old.get(i).copied().unwrap_or(false) {
                i += 1;
            }
            while rchg_new.get(j).copied().unwrap_or(false) {
                j += 1;
            }
            changes.push((start_i, i - start_i, start_j, j - start_j));
        } else {
            i += 1;
            j += 1;
        }
    }
    (changes, rchg_old, rchg_new)
}

/// Same as `unified_hunks`, but the indent heuristic can be turned off: without it, xdiff
/// leaves each sliding group at its lowest position (as git does when computing patch IDs).
// Used by `unified_hunks` and `cherry` patch IDs.
pub fn unified_hunks_with(old: &[String], new: &[String], indent_heuristic: bool) -> (String, usize, usize) {
    let (changes, rchg_old, rchg_new) = compacted_changes(old, new, indent_heuristic);

    let added = rchg_new.iter().filter(|&&c| c).count();
    let deleted = rchg_old.iter().filter(|&&c| c).count();
    let mut out = String::new();
    let mut func_line = String::new();
    let mut func_limit: i64 = -1;
    let mut c = 0;

    while c < changes.len() {
        let mut last = c;
        while last + 1 < changes.len() {
            let (pi, pc, _, _) = changes[last];
            let (ni, _, _, _) = changes[last + 1];
            if ni - (pi + pc) > 2 * CONTEXT_LINES {
                break;
            }
            last += 1;
        }
        let (first_i, _, first_j, _) = changes[c];
        let (last_i, last_ci, last_j, last_cj) = changes[last];
        let s1 = first_i.saturating_sub(CONTEXT_LINES);
        let s2 = first_j.saturating_sub(CONTEXT_LINES);
        let trailing = CONTEXT_LINES.min(old.len() - (last_i + last_ci)).min(new.len() - (last_j + last_cj));
        let e1 = last_i + last_ci + trailing;
        let e2 = last_j + last_cj + trailing;

        let start = s1 as i64 - 1;
        let mut l = start;
        while l != func_limit && l >= 0 && (l as usize) < old.len() {
            if let Some(found) = match_func_line(&old[l as usize]) {
                func_line = found;
                break;
            }
            l -= 1;
        }
        func_limit = start;

        let range = |s: usize, n: usize| -> String {
            let first = if n > 0 { s + 1 } else { s };
            if n == 1 { format!("{}", first) } else { format!("{},{}", first, n) }
        };
        out.push_str(&format!("@@ -{} +{} @@", range(s1, e1 - s1), range(s2, e2 - s2)));
        if !func_line.is_empty() {
            out.push(' ');
            out.push_str(&func_line);
        }
        out.push('\n');

        let (mut i, mut j) = (s1, s2);
        while i < e1 || j < e2 {
            let old_changed = i < e1 && rchg_old[i];
            let new_changed = j < e2 && rchg_new[j];
            if old_changed {
                out.push('-');
                out.push_str(&display_line(&old[i]));
                i += 1;
            } else if new_changed {
                out.push('+');
                out.push_str(&display_line(&new[j]));
                j += 1;
            } else {
                out.push(' ');
                out.push_str(&display_line(&new[j]));
                i += 1;
                j += 1;
            }
        }
        c = last + 1;
    }

    (out, added, deleted)
}

fn match_func_line(line: &str) -> Option<String> {
    let text = line.strip_suffix(NO_EOL_MARK).unwrap_or(line);
    let first = text.bytes().next()?;
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return None;
    }
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(b'\n');
    bytes.truncate(80);
    while bytes.last().is_some_and(|b| b.is_ascii_whitespace()) {
        bytes.pop();
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn get_indent(line: &str) -> i32 {
    let mut ret = 0;
    for c in line.strip_suffix(NO_EOL_MARK).unwrap_or(line).bytes() {
        if !matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            return ret;
        } else if c == b' ' {
            ret += 1;
        } else if c == b'\t' {
            ret += 8 - ret % 8;
        }
        if ret >= 200 {
            return 200;
        }
    }
    -1
}

fn split_score(lines: &[String], split: usize, effective_indent: &mut i32, penalty: &mut i32) {
    let end_of_file = split >= lines.len();
    let indent = if end_of_file { -1 } else { get_indent(&lines[split]) };

    let mut pre_blank = 0;
    let mut pre_indent = -1;
    let mut k = split as i64 - 1;
    while k >= 0 {
        pre_indent = get_indent(&lines[k as usize]);
        if pre_indent != -1 {
            break;
        }
        pre_blank += 1;
        if pre_blank == 20 {
            pre_indent = 0;
            break;
        }
        k -= 1;
    }

    let mut post_blank = 0;
    let mut post_indent = -1;
    for line in lines.iter().skip(split + 1) {
        post_indent = get_indent(line);
        if post_indent != -1 {
            break;
        }
        post_blank += 1;
        if post_blank == 20 {
            post_indent = 0;
            break;
        }
    }

    if pre_indent == -1 && pre_blank == 0 {
        *penalty += 1;
    }
    if end_of_file {
        *penalty += 21;
    }
    let post_blank_total = if indent == -1 { 1 + post_blank } else { 0 };
    let total_blank = pre_blank + post_blank_total;
    *penalty += -30 * total_blank;
    *penalty += 6 * post_blank_total;
    let indent = if indent != -1 { indent } else { post_indent };
    let any_blanks = total_blank != 0;
    *effective_indent += indent;

    if indent == -1 || pre_indent == -1 || indent == pre_indent {
    } else if indent > pre_indent {
        *penalty += if any_blanks { 10 } else { -4 };
    } else if post_indent != -1 && post_indent > indent {
        *penalty += if any_blanks { 17 } else { 24 };
    } else {
        *penalty += if any_blanks { 17 } else { 23 };
    }
}

struct Group {
    start: usize,
    end: usize,
}

fn changed(rchg: &[bool], i: i64) -> bool {
    i >= 0 && (i as usize) < rchg.len() && rchg[i as usize]
}

fn group_init(rchg: &[bool]) -> Group {
    let mut end = 0;
    while changed(rchg, end as i64) {
        end += 1;
    }
    Group { start: 0, end }
}

fn group_next(rchg: &[bool], g: &mut Group) -> bool {
    if g.end == rchg.len() {
        return false;
    }
    g.start = g.end + 1;
    g.end = g.start;
    while changed(rchg, g.end as i64) {
        g.end += 1;
    }
    true
}

fn group_previous(rchg: &[bool], g: &mut Group) -> bool {
    if g.start == 0 {
        return false;
    }
    g.end = g.start - 1;
    g.start = g.end;
    while changed(rchg, g.start as i64 - 1) {
        g.start -= 1;
    }
    true
}

fn group_slide_down(rchg: &mut [bool], lines: &[String], g: &mut Group) -> bool {
    if g.end < lines.len() && lines[g.start] == lines[g.end] {
        rchg[g.start] = false;
        rchg[g.end] = true;
        g.start += 1;
        g.end += 1;
        while changed(rchg, g.end as i64) {
            g.end += 1;
        }
        return true;
    }
    false
}

fn group_slide_up(rchg: &mut [bool], lines: &[String], g: &mut Group) -> bool {
    if g.start > 0 && lines[g.start - 1] == lines[g.end - 1] {
        g.start -= 1;
        g.end -= 1;
        rchg[g.start] = true;
        rchg[g.end] = false;
        while changed(rchg, g.start as i64 - 1) {
            g.start -= 1;
        }
        return true;
    }
    false
}

fn change_compact(rchg: &mut [bool], lines: &[String], rchgo: &mut [bool], indent_heuristic: bool) {
    let mut g = group_init(rchg);
    let mut go = group_init(rchgo);

    loop {
        if g.end != g.start {
            let mut earliest_end;
            let mut end_matching_other: i64;
            let mut groupsize;
            loop {
                groupsize = g.end - g.start;
                end_matching_other = -1;
                while group_slide_up(rchg, lines, &mut g) {
                    group_previous(rchgo, &mut go);
                }
                earliest_end = g.end;
                if go.end > go.start {
                    end_matching_other = g.end as i64;
                }
                while group_slide_down(rchg, lines, &mut g) {
                    group_next(rchgo, &mut go);
                    if go.end > go.start {
                        end_matching_other = g.end as i64;
                    }
                }
                if groupsize == g.end - g.start {
                    break;
                }
            }

            if g.end == earliest_end {
            } else if end_matching_other != -1 {
                while go.end == go.start {
                    group_slide_up(rchg, lines, &mut g);
                    group_previous(rchgo, &mut go);
                }
            } else if indent_heuristic {
                let mut shift = earliest_end as i64;
                if g.end as i64 - groupsize as i64 - 1 > shift {
                    shift = g.end as i64 - groupsize as i64 - 1;
                }
                if g.end as i64 - 100 > shift {
                    shift = g.end as i64 - 100;
                }
                let mut best_shift: i64 = -1;
                let mut best = (0i32, 0i32);
                while shift <= g.end as i64 {
                    let (mut indent, mut penalty) = (0, 0);
                    split_score(lines, shift as usize, &mut indent, &mut penalty);
                    split_score(lines, (shift - groupsize as i64) as usize, &mut indent, &mut penalty);
                    let cmp = 60 * ((indent > best.0) as i32 - (indent < best.0) as i32) + (penalty - best.1);
                    if best_shift == -1 || cmp <= 0 {
                        best = (indent, penalty);
                        best_shift = shift;
                    }
                    shift += 1;
                }
                while g.end as i64 > best_shift {
                    group_slide_up(rchg, lines, &mut g);
                    group_previous(rchgo, &mut go);
                }
            }
        }

        if !group_next(rchg, &mut g) {
            break;
        }
        group_next(rchgo, &mut go);
    }
}

/// Encodes one direction of a `GIT binary patch`: `literal <size>` followed by the zlib-deflated
/// data in base85 lines of up to 52 bytes (prefixed with a length letter), and a blank line.
// Used by `format-patch` via `diff_tree_file`.
pub fn encode_binary_literal(data: &[u8]) -> anyhow::Result<String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(data).context("Failed to deflate binary data")?;
    let deflated = encoder.finish()?;

    let mut out = format!("literal {}\n", data.len());
    for chunk in deflated.chunks(52) {
        let len_char = if chunk.len() <= 26 { b'A' + chunk.len() as u8 - 1 } else { b'a' + chunk.len() as u8 - 27 };
        out.push(len_char as char);
        for group in chunk.chunks(4) {
            let mut word = [0u8; 4];
            word[..group.len()].copy_from_slice(group);
            let mut value = u32::from_be_bytes(word);
            let mut encoded = [0u8; 5];
            for slot in encoded.iter_mut().rev() {
                *slot = BASE85_ALPHABET[(value % 85) as usize];
                value /= 85;
            }
            out.push_str(std::str::from_utf8(&encoded).unwrap_or_default());
        }
        out.push('\n');
    }
    out.push('\n');
    Ok(out)
}

/// Formats a diffstat block exactly like git's `--stat` for mail (72 columns): name column,
/// change count, scaled `+`/`-` graph (`Bin X -> Y bytes` for binaries), and the
/// `N files changed, X insertions(+), Y deletions(-)` summary.
// Used by `format-patch`.
pub fn format_diffstat(entries: &[DiffStatEntry], width: usize) -> String {
    let mut max_len = 0;
    let mut max_change = 0;
    let mut number_width = 0;
    let mut bin_width = 0;
    for e in entries {
        max_len = max_len.max(e.name.chars().count());
        if let Some((old, new)) = e.binary_sizes {
            bin_width = bin_width.max(14 + decimal_width(new) + decimal_width(old));
            number_width = 3;
            continue;
        }
        max_change = max_change.max(e.added + e.deleted);
    }
    number_width = number_width.max(decimal_width(max_change));
    let width = width.max(16 + 6 + number_width) as i64;
    let number_width_i = number_width as i64;

    let mut graph_width = if max_change + 4 > bin_width { max_change as i64 } else { bin_width as i64 - 4 };
    let mut name_width = max_len as i64;
    if name_width + number_width_i + 6 + graph_width > width {
        if graph_width > width * 3 / 8 - number_width_i - 6 {
            graph_width = (width * 3 / 8 - number_width_i - 6).max(6);
        }
        if name_width > width - number_width_i - 6 - graph_width {
            name_width = width - number_width_i - 6 - graph_width;
        } else {
            graph_width = width - number_width_i - 6 - name_width;
        }
    }

    let mut out = String::new();
    let (mut adds, mut dels) = (0, 0);
    for e in entries {
        let mut name: &str = &e.name;
        let mut prefix = "";
        let mut len = name_width;
        let name_len = name.chars().count() as i64;
        if name_width < name_len {
            prefix = "...";
            len = (len - 3).max(0);
            let skip = (name_len - len).max(0) as usize;
            let byte_index = name.char_indices().nth(skip).map(|(b, _)| b).unwrap_or(name.len());
            name = &name[byte_index..];
            if let Some(slash) = name.find('/') {
                name = &name[slash..];
            }
        }
        let padding = (len - name.chars().count() as i64).max(0) as usize;

        if let Some((old, new)) = e.binary_sizes {
            out.push_str(&format!(" {}{}{} | {:>w$}", prefix, name, " ".repeat(padding), "Bin", w = number_width));
            if old != 0 || new != 0 {
                out.push_str(&format!(" {} -> {} bytes", old, new));
            }
            out.push('\n');
            continue;
        }

        adds += e.added;
        dels += e.deleted;
        let (mut add, mut del) = (e.added as i64, e.deleted as i64);
        if graph_width <= max_change as i64 {
            let scale = |it: i64| if it == 0 { 0 } else { 1 + it * (graph_width - 1) / max_change as i64 };
            let mut total = scale(add + del);
            if total < 2 && add > 0 && del > 0 {
                total = 2;
            }
            if add < del {
                add = scale(add);
                del = total - add;
            } else {
                del = scale(del);
                add = total - del;
            }
        }
        let count = e.added + e.deleted;
        out.push_str(&format!(
            " {}{}{} | {:>w$}{}{}{}\n",
            prefix,
            name,
            " ".repeat(padding),
            count,
            if count > 0 { " " } else { "" },
            "+".repeat(add.max(0) as usize),
            "-".repeat(del.max(0) as usize),
            w = number_width
        ));
    }

    let files = entries.len();
    let mut summary = format!(" {} file{} changed", files, if files == 1 { "" } else { "s" });
    if adds > 0 || dels == 0 {
        summary.push_str(&format!(", {} insertion{}(+)", adds, if adds == 1 { "" } else { "s" }));
    }
    if dels > 0 || adds == 0 {
        summary.push_str(&format!(", {} deletion{}(-)", dels, if dels == 1 { "" } else { "s" }));
    }
    out.push_str(&summary);
    out.push('\n');
    out
}

fn decimal_width(n: usize) -> usize {
    n.to_string().len()
}
