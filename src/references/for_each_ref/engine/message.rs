//! Commit/tag message atoms: subject, body, signature, sanitized subject.

use crate::references::{
    for_each_ref::engine::trailers::format_message_trailers,
    for_each_ref::types::RefContentsPart,
};

/// Port of git's `find_subpos`: splits a commit/tag object into `(subject, body,
/// body-without-signature length, signature)` positions. `buf` is the whole object.
fn find_subpos(buf: &[u8]) -> (usize, usize, usize, usize, Vec<u8>) {
    let sig_in_whole = parse_signed_buffer(buf);
    let signature = buf[sig_in_whole..].to_vec();
    let mut pos = 0;
    while pos < buf.len() && buf[pos] != b'\n' {
        pos += buf[pos..].iter().position(|&b| b == b'\n').map(|p| p + 1).unwrap_or(buf.len() - pos);
    }
    while pos < buf.len() && buf[pos] == b'\n' {
        pos += 1;
    }
    let sigstart = pos + parse_signed_buffer(&buf[pos..]);
    let sub = pos;
    let find = |needle: &[u8]| buf[sub..].windows(needle.len()).position(|w| w == needle).map(|p| p + sub);
    let mut eol = match find(b"\n\n").or_else(|| find(b"\r\n\r\n")) {
        Some(e) => e.min(sigstart),
        None => sigstart,
    };
    let mut sublen = eol - sub;
    while sublen > 0 && matches!(buf[sub + sublen - 1], b'\n' | b'\r') {
        sublen -= 1;
    }
    while eol < buf.len() && matches!(buf[eol], b'\n' | b'\r') {
        eol += 1;
    }
    let body = eol;
    let nonsiglen = sigstart.saturating_sub(body);
    (sub, sublen, body, nonsiglen, signature)
}

fn parse_signed_buffer(buf: &[u8]) -> usize {
    const MARKERS: [&[u8]; 4] = [b"-----BEGIN PGP SIGNATURE-----", b"-----BEGIN PGP MESSAGE-----", b"-----BEGIN SIGNED MESSAGE-----", b"-----BEGIN SSH SIGNATURE-----"];
    let mut len = 0;
    let mut matched = buf.len();
    while len < buf.len() {
        if MARKERS.iter().any(|m| buf[len..].starts_with(m)) {
            matched = len;
        }
        len += buf[len..].iter().position(|&b| b == b'\n').map(|p| p + 1).unwrap_or(buf.len() - len);
    }
    matched
}

fn sanitized_subject(subject: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut space = 2;
    let mut i = 0;
    while i < subject.len() {
        let c = subject[i];
        if c.is_ascii_alphanumeric() || c == b'.' || c == b'_' {
            if space == 1 {
                out.push(b'-');
            }
            space = 0;
            out.push(c);
            if c == b'.' {
                while subject.get(i + 1) == Some(&b'.') {
                    i += 1;
                }
            }
        } else {
            space |= 1;
        }
        i += 1;
    }
    while matches!(out.last(), Some(b'.') | Some(b'-')) {
        out.pop();
    }
    out
}

pub(super) fn grab_message_value(buf: &[u8], part: &RefContentsPart) -> (Vec<u8>, u64) {
    let (sub, sublen, body, nonsiglen, signature) = find_subpos(buf);
    match part {
        RefContentsPart::Subject => {
            let mut out = Vec::new();
            let s = &buf[sub..sub + sublen];
            let mut i = 0;
            while i < s.len() {
                if s[i] == b'\r' && s.get(i + 1) == Some(&b'\n') {
                    i += 1;
                    continue;
                }
                out.push(if s[i] == b'\n' { b' ' } else { s[i] });
                i += 1;
            }
            (out, 0)
        }
        RefContentsPart::SubjectSanitized => (sanitized_subject(&buf[sub..sub + sublen]), 0),
        RefContentsPart::BodyWithSignature => (buf[body..].to_vec(), 0),
        RefContentsPart::Size => {
            let n = (buf.len() - sub) as u64;
            (n.to_string().into_bytes(), n)
        }
        RefContentsPart::Body => (buf[body..body + nonsiglen].to_vec(), 0),
        RefContentsPart::Signature => (signature, 0),
        RefContentsPart::Lines(n) => {
            let text = &buf[sub..body + nonsiglen];
            let mut out = Vec::new();
            let mut sp = 0;
            for i in 0..*n {
                if sp >= text.len() {
                    break;
                }
                if i > 0 {
                    out.extend_from_slice(b"\n    ");
                }
                match text[sp..].iter().position(|&b| b == b'\n') {
                    Some(eol) => {
                        out.extend_from_slice(&text[sp..sp + eol]);
                        sp += eol + 1;
                    }
                    None => {
                        out.extend_from_slice(&text[sp..]);
                        break;
                    }
                }
            }
            (out, 0)
        }
        RefContentsPart::Trailers(opts) => (format_message_trailers(&buf[sub..], opts), 0),
        RefContentsPart::Bare => (buf[sub..].to_vec(), 0),
    }
}
