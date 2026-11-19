//! `%(trailers)`: trailer block detection, unfolding and formatting (ref-filter's port of trailer.c).

use crate::references::for_each_ref::types::TrailerFormatOptions;

fn is_blank_line(line: &[u8]) -> bool {
    line.iter().take_while(|&&b| b != b'\n').all(|b| b.is_ascii_whitespace())
}

fn next_line(buf: &[u8], pos: usize) -> usize {
    match buf[pos..].iter().position(|&b| b == b'\n') {
        Some(p) => pos + p + 1,
        None => buf.len(),
    }
}

fn last_line(buf: &[u8], len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if len == 1 {
        return Some(0);
    }
    (0..=len - 2).rev().find(|&i| buf[i] == b'\n').map(|i| i + 1).or(Some(0))
}

fn find_trailer_separator(line: &[u8]) -> Option<usize> {
    let mut whitespace_found = false;
    for (i, &c) in line.iter().enumerate() {
        if c == b':' {
            return Some(i);
        }
        if !whitespace_found && (c.is_ascii_alphanumeric() || c == b'-') {
            continue;
        }
        if i != 0 && (c == b' ' || c == b'\t') {
            whitespace_found = true;
            continue;
        }
        break;
    }
    None
}

fn find_trailer_start(buf: &[u8], len: usize) -> usize {
    let mut s = 0;
    while s < len {
        if buf[s] == b'#' {
            s = next_line(buf, s);
            continue;
        }
        if is_blank_line(&buf[s..]) {
            break;
        }
        s = next_line(buf, s);
    }
    let end_of_title = s;
    let (mut only_spaces, mut recognized_prefix) = (true, false);
    let (mut trailer_lines, mut non_trailer_lines, mut possible_continuation_lines) = (0, 0, 0);
    let mut l = last_line(buf, len);
    while let Some(bol) = l {
        if bol < end_of_title {
            break;
        }
        let line = &buf[bol..];
        if line.first() == Some(&b'#') {
            non_trailer_lines += possible_continuation_lines;
            possible_continuation_lines = 0;
            l = last_line(buf, bol);
            continue;
        }
        if is_blank_line(line) {
            if only_spaces {
                l = last_line(buf, bol);
                continue;
            }
            non_trailer_lines += possible_continuation_lines;
            if recognized_prefix && trailer_lines * 3 >= non_trailer_lines {
                return next_line(buf, bol);
            } else if trailer_lines > 0 && non_trailer_lines == 0 {
                return next_line(buf, bol);
            }
            return len;
        }
        only_spaces = false;
        if line.starts_with(b"Signed-off-by: ") || line.starts_with(b"(cherry picked from commit ") {
            trailer_lines += 1;
            possible_continuation_lines = 0;
            recognized_prefix = true;
        } else if find_trailer_separator(line).is_some_and(|p| p >= 1) && !line[0].is_ascii_whitespace() {
            trailer_lines += 1;
            possible_continuation_lines = 0;
        } else if line[0].is_ascii_whitespace() {
            possible_continuation_lines += 1;
        } else {
            non_trailer_lines += 1 + possible_continuation_lines;
            possible_continuation_lines = 0;
        }
        l = last_line(buf, bol);
    }
    len
}

fn ignore_non_trailer(buf: &[u8], len: usize) -> usize {
    let cut = b"\n# ------------------------ >8 ------------------------";
    let cutoff = if buf.starts_with(&cut[1..]) {
        0
    } else {
        buf[..len].windows(cut.len()).position(|w| w == cut).map(|p| p + 1).unwrap_or(len)
    };
    let (mut boc, mut bol, mut in_conflicts) = (0, 0, false);
    while bol < cutoff {
        let next = next_line(&buf[..len], bol);
        if buf[bol] == b'#' || buf[bol] == b'\n' {
            if boc == 0 {
                boc = bol;
            }
        } else if buf[bol..].starts_with(b"Conflicts:\n") {
            in_conflicts = true;
            if boc == 0 {
                boc = bol;
            }
        } else if in_conflicts && buf[bol] == b'\t' {
        } else if boc != 0 {
            boc = 0;
            in_conflicts = false;
        }
        bol = next;
    }
    if boc != 0 { len - boc } else { len - cutoff }
}

fn trim_bytes(b: &[u8]) -> &[u8] {
    let start = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !c.is_ascii_whitespace()).map(|p| p + 1).unwrap_or(start);
    &b[start..end.max(start)]
}

fn unfold_value(val: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < val.len() {
        let c = val[i];
        i += 1;
        if c == b'\n' {
            while i < val.len() && val[i].is_ascii_whitespace() {
                i += 1;
            }
            out.push(b' ');
        } else {
            out.push(c);
        }
    }
    trim_bytes(&out).to_vec()
}

/// Formats a message's trailer block like git's `format_trailers_from_commit` (with
/// `no_divider`, as `for-each-ref` uses it): the raw block when no option changes it, otherwise
/// one trailer per line (or `separator`-joined), filtered, unfolded and split as asked.
// Used by `for-each-ref` (`%(trailers)`, `%(contents:trailers)`).
pub fn format_message_trailers(msg: &[u8], opts: &TrailerFormatOptions) -> Vec<u8> {
    let msg = &msg[..msg.iter().position(|&b| b == 0).unwrap_or(msg.len())];
    let trailer_end = msg.len() - ignore_non_trailer(msg, msg.len());
    let trailer_start = find_trailer_start(msg, trailer_end);
    let block = &msg[trailer_start..trailer_end.max(trailer_start)];

    let mut trailers: Vec<Vec<u8>> = Vec::new();
    let mut last_is_trailer = false;
    let mut pos = 0;
    while pos < block.len() {
        let end = next_line(block, pos);
        let line = &block[pos..end];
        if last_is_trailer && line.first().is_some_and(|c| c.is_ascii_whitespace()) {
            if let Some(last) = trailers.last_mut() {
                last.extend_from_slice(line);
            }
        } else {
            last_is_trailer = find_trailer_separator(line).is_some_and(|p| p >= 1);
            trailers.push(line.to_vec());
        }
        pos = end;
    }

    let plain = !opts.only && !opts.unfold && opts.keys.is_empty() && opts.separator.is_none() && !opts.key_only && !opts.value_only && opts.key_value_separator.is_none();
    if plain {
        return block.to_vec();
    }
    let mut out: Vec<u8> = Vec::new();
    for trailer in &trailers {
        match find_trailer_separator(trailer).filter(|p| *p >= 1) {
            Some(sep) => {
                let tok = trim_bytes(&trailer[..sep]).to_vec();
                let mut val = trim_bytes(&trailer[sep + 1..]).to_vec();
                let keep = opts.keys.is_empty() || opts.keys.iter().any(|k| k.len() == tok.len() && k.as_bytes().eq_ignore_ascii_case(&tok));
                if !keep {
                    continue;
                }
                if opts.unfold {
                    val = unfold_value(&val);
                }
                if let Some(sep) = &opts.separator {
                    if !out.is_empty() {
                        out.extend_from_slice(sep);
                    }
                }
                if !opts.value_only {
                    out.extend_from_slice(&tok);
                }
                if !opts.key_only && !opts.value_only {
                    match &opts.key_value_separator {
                        Some(kv) => out.extend_from_slice(kv),
                        None => out.extend_from_slice(b": "),
                    }
                }
                if !opts.key_only {
                    out.extend_from_slice(&val);
                }
                if opts.separator.is_none() {
                    out.push(b'\n');
                }
            }
            None if !opts.only => {
                if let Some(sep) = &opts.separator {
                    if !out.is_empty() {
                        out.extend_from_slice(sep);
                    }
                }
                out.extend_from_slice(trailer);
                if opts.separator.is_some() {
                    while out.last().is_some_and(|c| c.is_ascii_whitespace()) {
                        out.pop();
                    }
                }
            }
            None => {}
        }
    }
    out
}
