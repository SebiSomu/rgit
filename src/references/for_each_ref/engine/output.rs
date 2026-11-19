//! Output: quoting styles, display width, `%(align)`/`%(if)` stack, and `format_ref_item`.

use crate::references::{
    for_each_ref::engine::COLOR_RESET,
    for_each_ref::engine::atoms::hex_pair,
    for_each_ref::types::{RefAlign, RefAtomArg, RefAtomKind, RefAtomValue, RefFilterAtom, RefFormatPiece, RefQuoteStyle},
};

pub(super) fn c_string(s: &[u8]) -> &[u8] {
    &s[..s.iter().position(|&b| b == 0).unwrap_or(s.len())]
}

/// Appends `s` quoted for `style` like git's `quote_formatting` (shell `'...'` with `'\''`,
/// Perl/Python `'...'` with backslashes, Tcl `"..."`); shell, Python and Tcl stop at a NUL.
// Used by `for-each-ref`.
pub fn quote_ref_value(out: &mut Vec<u8>, s: &[u8], style: RefQuoteStyle) {
    match style {
        RefQuoteStyle::None => out.extend_from_slice(s),
        RefQuoteStyle::Shell => {
            out.push(b'\'');
            for &c in c_string(s) {
                if c == b'\'' || c == b'!' {
                    out.extend_from_slice(b"'\\");
                    out.push(c);
                    out.push(b'\'');
                } else {
                    out.push(c);
                }
            }
            out.push(b'\'');
        }
        RefQuoteStyle::Perl | RefQuoteStyle::Python => {
            let src = if style == RefQuoteStyle::Perl { s } else { c_string(s) };
            out.push(b'\'');
            for &c in src {
                if style == RefQuoteStyle::Python && c == b'\n' {
                    out.extend_from_slice(b"\\n");
                    continue;
                }
                if c == b'\'' || c == b'\\' {
                    out.push(b'\\');
                }
                out.push(c);
            }
            out.push(b'\'');
        }
        RefQuoteStyle::Tcl => {
            out.push(b'"');
            for &c in c_string(s) {
                match c {
                    b'[' | b']' | b'{' | b'}' | b'$' | b'\\' | b'"' => {
                        out.push(b'\\');
                        out.push(c);
                    }
                    0x0c => out.extend_from_slice(b"\\f"),
                    b'\r' => out.extend_from_slice(b"\\r"),
                    b'\n' => out.extend_from_slice(b"\\n"),
                    b'\t' => out.extend_from_slice(b"\\t"),
                    0x0b => out.extend_from_slice(b"\\v"),
                    _ => out.push(c),
                }
            }
            out.push(b'"');
        }
    }
}

fn display_width(s: &[u8]) -> usize {
    let Ok(text) = std::str::from_utf8(s) else { return s.len() };
    text.chars()
        .map(|c| {
            let u = c as u32;
            if u < 0x20 || (0x7f..0xa0).contains(&u) {
                0
            } else if (0x300..0x370).contains(&u) || (0x200b..0x2010).contains(&u) || (0xfe00..0xfe10).contains(&u) {
                0
            } else if (0x1100..0x1160).contains(&u)
                || (0x2e80..0xa4d0).contains(&u)
                || (0xac00..0xd7a4).contains(&u)
                || (0xf900..0xfb00).contains(&u)
                || (0xfe30..0xfe50).contains(&u)
                || (0xff00..0xff61).contains(&u)
                || (0xffe0..0xffe7).contains(&u)
                || (0x1f300..0x1fa00).contains(&u)
                || (0x20000..0x3fffe).contains(&u)
            {
                2
            } else {
                1
            }
        })
        .sum()
}

fn align_ref_text(s: &[u8], position: RefAlign, width: usize) -> Vec<u8> {
    let s = c_string(s);
    let len = display_width(s);
    if len >= width {
        return s.to_vec();
    }
    let pad = width - len;
    let (left, right) = match position {
        RefAlign::Left => (0, pad),
        RefAlign::Right => (pad, 0),
        RefAlign::Middle => (pad / 2, pad - pad / 2),
    };
    let mut out = vec![b' '; left];
    out.extend_from_slice(s);
    out.extend(std::iter::repeat_n(b' ', right));
    out
}

fn append_literal(out: &mut Vec<u8>, text: &str) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if bytes.get(i + 1) == Some(&b'%') {
                i += 1;
            } else if let Some(ch) = hex_pair(&bytes[i + 1..]) {
                out.push(ch);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
}

enum StackEnd {
    Base,
    Align(RefAlign, usize),
    If(usize),
}

struct IfState {
    cmp: Option<(bool, String)>,
    then_seen: bool,
    else_seen: bool,
    satisfied: bool,
}

/// Formats one ref with the parsed format, like git's `format_ref_array_item`: literals,
/// quoted values, the `%(align)` / `%(if)...%(then)...%(else)...%(end)` stack (quoting the
/// whole block at `%(end)` when nested at top level) and a color reset at the end of the line.
/// Errors are git's fatal messages.
// Used by `for-each-ref`.
pub fn format_ref_item(
    pieces: &[RefFormatPiece],
    atoms: &[RefFilterAtom],
    values: &[RefAtomValue],
    quote: RefQuoteStyle,
    need_reset: bool,
) -> Result<Vec<u8>, String> {
    let mut stack: Vec<(Vec<u8>, StackEnd)> = vec![(Vec::new(), StackEnd::Base)];
    let mut ifs: Vec<IfState> = Vec::new();

    let append = |stack: &mut Vec<(Vec<u8>, StackEnd)>, v: &RefAtomValue| {
        let top_level = stack.len() == 1;
        let out = &mut stack.last_mut().expect("stack").0;
        if top_level {
            let s = if v.binary { &v.s[..] } else { c_string(&v.s) };
            quote_ref_value(out, s, quote);
        } else if v.binary {
            out.extend_from_slice(&v.s);
        } else {
            out.extend_from_slice(c_string(&v.s));
        }
    };

    for (literal, atom) in pieces {
        if let Some(text) = literal {
            append_literal(&mut stack.last_mut().expect("stack").0, text);
            continue;
        }
        let Some(at) = *atom else { continue };
        let atom = &atoms[at];
        let value = &values[at];
        match atom.kind {
            RefAtomKind::Align => {
                let RefAtomArg::Align { position, width } = atom.arg else { continue };
                stack.push((Vec::new(), StackEnd::Align(position, width)));
            }
            RefAtomKind::If => {
                let RefAtomArg::If(cmp) = &atom.arg else { continue };
                ifs.push(IfState { cmp: cmp.clone(), then_seen: false, else_seen: false, satisfied: false });
                stack.push((Vec::new(), StackEnd::If(ifs.len() - 1)));
            }
            RefAtomKind::Then => {
                let idx = match stack.last() {
                    Some((_, StackEnd::If(idx))) => *idx,
                    _ => return Err("format: %(then) atom used without a %(if) atom".to_string()),
                };
                let state = &mut ifs[idx];
                if state.then_seen {
                    return Err("format: %(then) atom used more than once".to_string());
                }
                if state.else_seen {
                    return Err("format: %(then) atom used after %(else)".to_string());
                }
                state.then_seen = true;
                let out = &mut stack.last_mut().expect("stack").0;
                state.satisfied = match &state.cmp {
                    Some((true, s)) => s.as_bytes() == &out[..],
                    Some((false, s)) => s.as_bytes() != &out[..],
                    None => !out.is_empty() && !out.iter().all(|c| c.is_ascii_whitespace()),
                };
                out.clear();
            }
            RefAtomKind::Else => {
                let idx = match stack.last() {
                    Some((_, StackEnd::If(idx))) => *idx,
                    _ => return Err("format: %(else) atom used without a %(if) atom".to_string()),
                };
                let state = &mut ifs[idx];
                if !state.then_seen {
                    return Err("format: %(else) atom used without a %(then) atom".to_string());
                }
                if state.else_seen {
                    return Err("format: %(else) atom used more than once".to_string());
                }
                state.else_seen = true;
                stack.push((Vec::new(), StackEnd::If(idx)));
            }
            RefAtomKind::End => {
                match stack.last() {
                    Some((_, StackEnd::Align(position, width))) => {
                        let (position, width) = (*position, *width);
                        let top = stack.last_mut().expect("stack");
                        top.0 = align_ref_text(&top.0, position, width);
                    }
                    Some((_, StackEnd::If(idx))) => {
                        let idx = *idx;
                        let state = &ifs[idx];
                        if !state.then_seen {
                            return Err("format: %(if) atom used without a %(then) atom".to_string());
                        }
                        if state.else_seen {
                            let (cur, _) = stack.pop().expect("stack");
                            if !state.satisfied {
                                stack.last_mut().expect("stack").0 = cur;
                            }
                        } else if !state.satisfied {
                            stack.last_mut().expect("stack").0.clear();
                        }
                    }
                    _ => return Err("format: %(end) atom used without corresponding atom".to_string()),
                }
                if stack.len() == 2 {
                    let top = stack.last_mut().expect("stack");
                    let mut quoted = Vec::new();
                    quote_ref_value(&mut quoted, &top.0, quote);
                    top.0 = quoted;
                }
                let (done, _) = stack.pop().expect("stack");
                stack.last_mut().expect("stack").0.extend_from_slice(&done);
            }
            _ => append(&mut stack, value),
        }
    }
    if need_reset {
        append(&mut stack, &RefAtomValue { s: COLOR_RESET.as_bytes().to_vec(), ..Default::default() });
    }
    if stack.len() > 1 {
        return Err("format: %(end) atom missing".to_string());
    }
    Ok(stack.pop().map(|(s, _)| s).unwrap_or_default())
}
