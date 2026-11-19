//! git's `color_parse`: color specifications to ANSI escape sequences for `%(color:...)`.

use crate::references::for_each_ref::engine::atoms::hex_pair;

/// Turns a git color specification (`red`, `bold blue`, `#ff0000 ul`, `reset`, `223`...) into
/// its ANSI escape sequence, like git's `color_parse`; `Err` for invalid values.
// Used by `for-each-ref` (`%(color:...)`).
pub fn parse_color(value: &str) -> Result<String, ()> {
    #[derive(Clone, Copy, PartialEq)]
    enum C {
        Unspecified,
        Normal,
        Ansi(u8),
        C256(u8),
        Rgb(u8, u8, u8),
    }
    let parse_one = |word: &str| -> Option<C> {
        let lower = word.to_ascii_lowercase();
        if lower == "normal" {
            return Some(C::Normal);
        }
        if word.len() == 7 && word.starts_with('#') {
            if let (Some(r), Some(g), Some(b)) = (hex_pair(&word.as_bytes()[1..]), hex_pair(&word.as_bytes()[3..]), hex_pair(&word.as_bytes()[5..])) {
                return Some(C::Rgb(r, g, b));
            }
        }
        if lower == "default" {
            return Some(C::Ansi(39));
        }
        let names = ["black", "red", "green", "yellow", "blue", "magenta", "cyan", "white"];
        let (offset, base) = match lower.strip_prefix("bright") {
            Some(rest) => (90, rest),
            None => (30, lower.as_str()),
        };
        if let Some(i) = names.iter().position(|n| *n == base) {
            return Some(C::Ansi(offset + i as u8));
        }
        let n: i64 = word.parse().ok()?;
        match n {
            -1 => Some(C::Normal),
            0..=7 => Some(C::Ansi(30 + n as u8)),
            8..=15 => Some(C::Ansi(90 + n as u8 - 8)),
            16..=255 => Some(C::C256(n as u8)),
            _ => None,
        }
    };
    let parse_attr = |word: &str| -> Option<u32> {
        let (negate, name) = match word.strip_prefix("no") {
            Some(rest) => (true, rest.strip_prefix('-').unwrap_or(rest)),
            None => (false, word),
        };
        let (val, neg) = match name {
            "bold" => (1, 22),
            "dim" => (2, 22),
            "italic" => (3, 23),
            "ul" => (4, 24),
            "blink" => (5, 25),
            "reverse" => (7, 27),
            "strike" => (9, 29),
            _ => return None,
        };
        Some(if negate { neg } else { val })
    };
    let output = |c: C, background: bool| -> String {
        let off = if background { 10 } else { 0 };
        match c {
            C::Unspecified | C::Normal => String::new(),
            C::Ansi(v) => format!("{}", v as u32 + off),
            C::C256(v) => format!("{};5;{}", 38 + off, v),
            C::Rgb(r, g, b) => format!("{};2;{};{};{}", 38 + off, r, g, b),
        }
    };

    let words: Vec<&str> = value.split(|c: char| c.is_ascii_whitespace()).filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return Ok(String::new());
    }
    let (mut has_reset, mut attr, mut fg, mut bg) = (false, 0u64, C::Unspecified, C::Unspecified);
    for word in words {
        if word.eq_ignore_ascii_case("reset") {
            has_reset = true;
            continue;
        }
        if let Some(c) = parse_one(word) {
            if fg == C::Unspecified {
                fg = c;
            } else if bg == C::Unspecified {
                bg = c;
            } else {
                return Err(());
            }
            continue;
        }
        match parse_attr(word) {
            Some(v) => attr |= 1 << v,
            None => return Err(()),
        }
    }
    let empty = |c: C| matches!(c, C::Unspecified | C::Normal);
    if !(has_reset || attr != 0 || !empty(fg) || !empty(bg)) {
        return Ok(String::new());
    }
    let mut out = String::from("\x1b[");
    let mut sep = usize::from(has_reset);
    for i in 0..64 {
        if attr & (1 << i) != 0 {
            if sep > 0 {
                out.push(';');
            }
            sep += 1;
            out.push_str(&i.to_string());
        }
    }
    for (c, background) in [(fg, false), (bg, true)] {
        if !empty(c) {
            if sep > 0 {
                out.push(';');
            }
            sep += 1;
            out.push_str(&output(c, background));
        }
    }
    out.push('m');
    Ok(out)
}
