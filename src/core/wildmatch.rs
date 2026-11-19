/// `wildmatch` flag: `*` and `?` don't match `/`, and `**` matches across directories.
pub const WM_PATHNAME: u32 = 2;
/// `wildmatch` flag: compare ASCII letters case-insensitively.
pub const WM_CASEFOLD: u32 = 1;

const WM_MATCH: i32 = 0;
const WM_NOMATCH: i32 = 1;
const WM_ABORT_ALL: i32 = -1;
const WM_ABORT_TO_STARSTAR: i32 = -2;

/// Matches `text` against a shell glob exactly like git's `wildmatch()`: `?`, `*`, `[...]`
/// classes (with `!`/`^` negation, ranges and `[:alpha:]`-style names) and `\` escapes; with
/// `WM_PATHNAME`, `*` stops at `/` and `**` spans directories; `WM_CASEFOLD` ignores ASCII case.
// Used by `for-each-ref` (ref patterns, `--exclude`, `%(describe:match=...)`).
pub fn wildmatch(pattern: &str, text: &str, flags: u32) -> bool {
    dowild(pattern.as_bytes(), 0, text.as_bytes(), 0, flags) == WM_MATCH
}

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

fn is_glob_special(c: u8) -> bool {
    matches!(c, b'*' | b'?' | b'[' | b'\\')
}

fn dowild(pat: &[u8], mut p: usize, text: &[u8], mut t: usize, flags: u32) -> i32 {
    let fold = |c: u8| if flags & WM_CASEFOLD != 0 { c.to_ascii_lowercase() } else { c };
    while at(pat, p) != 0 {
        let mut p_ch = at(pat, p);
        let mut t_ch = at(text, t);
        if t_ch == 0 && p_ch != b'*' {
            return WM_ABORT_ALL;
        }
        t_ch = fold(t_ch);
        p_ch = fold(p_ch);
        match p_ch {
            b'?' => {
                if flags & WM_PATHNAME != 0 && t_ch == b'/' {
                    return WM_NOMATCH;
                }
            }
            b'*' => {
                let match_slash;
                p += 1;
                if at(pat, p) == b'*' {
                    let prev_p = p;
                    while at(pat, p + 1) == b'*' {
                        p += 1;
                    }
                    p += 1;
                    if flags & WM_PATHNAME == 0 {
                        match_slash = true;
                    } else if (prev_p < 2 || at(pat, prev_p - 2) == b'/')
                        && (at(pat, p) == 0 || at(pat, p) == b'/' || (at(pat, p) == b'\\' && at(pat, p + 1) == b'/'))
                    {
                        if at(pat, p) == b'/' && dowild(pat, p + 1, text, t, flags) == WM_MATCH {
                            return WM_MATCH;
                        }
                        match_slash = true;
                    } else {
                        match_slash = false;
                    }
                } else {
                    match_slash = flags & WM_PATHNAME == 0;
                }
                if at(pat, p) == 0 {
                    if !match_slash && text[t..].contains(&b'/') {
                        return WM_ABORT_TO_STARSTAR;
                    }
                    return WM_MATCH;
                } else if !match_slash && at(pat, p) == b'/' {
                    match text[t..].iter().position(|&c| c == b'/') {
                        Some(off) => {
                            t += off;
                            t += 1;
                            p += 1;
                            continue;
                        }
                        None => return WM_ABORT_ALL,
                    }
                }
                let mut t_ch = at(text, t);
                loop {
                    if t_ch == 0 {
                        break;
                    }
                    if !is_glob_special(at(pat, p)) {
                        let lit = fold(at(pat, p));
                        loop {
                            t_ch = at(text, t);
                            if t_ch == 0 || (!match_slash && t_ch == b'/') {
                                break;
                            }
                            t_ch = fold(t_ch);
                            if t_ch == lit {
                                break;
                            }
                            t += 1;
                        }
                        if t_ch != lit {
                            return if match_slash { WM_ABORT_ALL } else { WM_ABORT_TO_STARSTAR };
                        }
                    }
                    let matched = dowild(pat, p, text, t, flags);
                    if matched != WM_NOMATCH {
                        if !match_slash || matched != WM_ABORT_TO_STARSTAR {
                            return matched;
                        }
                    } else if !match_slash && t_ch == b'/' {
                        return WM_ABORT_TO_STARSTAR;
                    }
                    t += 1;
                    t_ch = at(text, t);
                }
                return WM_ABORT_ALL;
            }
            b'[' => {
                p += 1;
                let mut p_ch = at(pat, p);
                if p_ch == b'^' {
                    p_ch = b'!';
                }
                let negated = p_ch == b'!';
                if negated {
                    p += 1;
                    p_ch = at(pat, p);
                }
                let mut prev_ch: u8 = 0;
                let mut matched = false;
                loop {
                    if p_ch == 0 {
                        return WM_ABORT_ALL;
                    }
                    if p_ch == b'\\' {
                        p += 1;
                        p_ch = at(pat, p);
                        if p_ch == 0 {
                            return WM_ABORT_ALL;
                        }
                        if t_ch == p_ch {
                            matched = true;
                        }
                    } else if p_ch == b'-' && prev_ch != 0 && at(pat, p + 1) != 0 && at(pat, p + 1) != b']' {
                        p += 1;
                        p_ch = at(pat, p);
                        if p_ch == b'\\' {
                            p += 1;
                            p_ch = at(pat, p);
                            if p_ch == 0 {
                                return WM_ABORT_ALL;
                            }
                        }
                        if t_ch <= p_ch && t_ch >= prev_ch {
                            matched = true;
                        } else if flags & WM_CASEFOLD != 0 && t_ch.is_ascii_lowercase() {
                            let upper = t_ch.to_ascii_uppercase();
                            if upper <= p_ch && upper >= prev_ch {
                                matched = true;
                            }
                        }
                        p_ch = 0;
                    } else if p_ch == b'[' && at(pat, p + 1) == b':' {
                        p += 2;
                        let s = p;
                        while at(pat, p) != 0 && at(pat, p) != b']' {
                            p += 1;
                        }
                        if at(pat, p) == 0 {
                            return WM_ABORT_ALL;
                        }
                        if p == s || at(pat, p - 1) != b':' {
                            p = s - 2;
                            p_ch = b'[';
                            if t_ch == p_ch {
                                matched = true;
                            }
                        } else {
                            let class = &pat[s..p - 1];
                            let hit = match class {
                                b"alnum" => t_ch.is_ascii_alphanumeric(),
                                b"alpha" => t_ch.is_ascii_alphabetic(),
                                b"blank" => t_ch == b' ' || t_ch == b'\t',
                                b"cntrl" => t_ch.is_ascii_control(),
                                b"digit" => t_ch.is_ascii_digit(),
                                b"graph" => t_ch.is_ascii_graphic(),
                                b"lower" => t_ch.is_ascii_lowercase(),
                                b"print" => t_ch.is_ascii_graphic() || t_ch == b' ',
                                b"punct" => t_ch.is_ascii_punctuation(),
                                b"space" => matches!(t_ch, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c),
                                b"upper" => t_ch.is_ascii_uppercase() || (flags & WM_CASEFOLD != 0 && t_ch.is_ascii_lowercase()),
                                b"xdigit" => t_ch.is_ascii_hexdigit(),
                                _ => return WM_ABORT_ALL,
                            };
                            if hit {
                                matched = true;
                            }
                            p_ch = 0;
                        }
                    } else if t_ch == p_ch {
                        matched = true;
                    }
                    prev_ch = p_ch;
                    p += 1;
                    p_ch = at(pat, p);
                    if p_ch == b']' {
                        break;
                    }
                }
                if matched == negated || (flags & WM_PATHNAME != 0 && t_ch == b'/') {
                    return WM_NOMATCH;
                }
            }
            _ => {
                let lit = if p_ch == b'\\' {
                    p += 1;
                    at(pat, p)
                } else {
                    p_ch
                };
                if t_ch != lit {
                    return WM_NOMATCH;
                }
            }
        }
        t += 1;
        p += 1;
    }
    if at(text, t) != 0 { WM_NOMATCH } else { WM_MATCH }
}
