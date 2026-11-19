//! Format preparation: atom table, atom lookup, and parsing of each atom's arguments (ref-filter.c's `parse_ref_filter_atom`).

use crate::core::{config::get_config_value, object_store::read_object, rev_parse::resolve_rev};
use crate::references::{
    for_each_ref::engine::{fatal, usage_error},
    for_each_ref::engine::color::parse_color,
    for_each_ref::engine::refs::resolve_head_ref,
    for_each_ref::types::{ForEachRefExit, ForEachRefOptions, RefAlign, RefAtomArg, RefAtomKind, RefContentsPart, RefFilterAtom, RefFormatPiece, RefnameShow, RefQuoteStyle, RemoteRefShow, TrailerFormatOptions},
};
use std::io::IsTerminal;

/// Peels `oid` through tags to a commit, or explains (like git's
/// `lookup_commit_reference`) which object was in the way.
pub(super) fn peel_to_commit_reporting(oid: &str) -> Result<String, String> {
    let mut hash = oid.to_string();
    loop {
        let (kind, content) = read_object(&hash).map_err(|_| format!("error: unable to read {}", hash))?;
        match kind.as_str() {
            "commit" => return Ok(hash),
            "tag" => match tag_target(&content) {
                Some(target) => hash = target,
                None => return Err(format!("error: object {} is a tag, not a commit", hash)),
            },
            other => return Err(format!("error: object {} is a {}, not a commit", hash, other)),
        }
    }
}

pub(super) fn peel_to_commit(oid: &str) -> Option<String> {
    peel_to_commit_reporting(oid).ok()
}

pub(super) fn tag_target(content: &[u8]) -> Option<String> {
    header_value(content, "object").filter(|h| h.len() == 40)
}

pub(super) fn header_value(content: &[u8], key: &str) -> Option<String> {
    for line in content.split(|&b| b == b'\n') {
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix(key.as_bytes()).and_then(|r| r.strip_prefix(b" ")) {
            return Some(String::from_utf8_lossy(rest).to_string());
        }
    }
    None
}

/// Says whether `for-each-ref` should emit colors: `--color`/`--no-color` decide, otherwise
/// `color.ui` (`always`, `never`, or the default `auto`, meaning stdout is a terminal).
// Used by `for-each-ref`.
pub fn want_ref_color(forced: Option<bool>) -> bool {
    if let Some(forced) = forced {
        return forced;
    }
    let setting = get_config_value("color.ui").ok().flatten().map(|v| v.to_ascii_lowercase());
    match setting.as_deref() {
        Some("never") | Some("false") | Some("no") | Some("off") | Some("0") => false,
        Some("always") => true,
        _ => std::io::stdout().is_terminal() && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true),
    }
}

/// Checks a `for-each-ref` format like git's `verify_ref_format` and parses its atoms and the
/// `--sort` keys (into `atoms`, shared and de-duplicated by text). Returns the format pieces,
/// whether a color reset is needed at the end of each line, and the sort keys as
/// `(atom, reverse, version)` with the primary key first.
// Used by `for-each-ref`.
pub fn prepare_ref_format(
    opts: &ForEachRefOptions,
    use_color: bool,
    atoms: &mut Vec<RefFilterAtom>,
) -> Result<(Vec<RefFormatPiece>, bool, Vec<(usize, bool, bool)>), ForEachRefExit> {
    let format = opts.format.as_str();
    let mut pieces = Vec::new();
    let mut need_reset = false;
    let mut cp = 0;
    while let Some(sp) = find_next_atom(format, cp) {
        if sp > cp {
            pieces.push((Some(format[cp..sp].to_string()), None));
        }
        let Some(ep) = format[sp..].find(')').map(|p| sp + p) else {
            return Err(usage_error(format!("error: malformed format string {}", &format[sp..])));
        };
        let text = &format[sp + 2..ep];
        let at = parse_ref_atom(atoms, text, use_color).map_err(fatal)?;
        if atoms[at].kind == RefAtomKind::Rest {
            return Err(fatal(format!("this command reject atom %({})", text)));
        }
        if matches!(opts.quote, RefQuoteStyle::Python | RefQuoteStyle::Shell | RefQuoteStyle::Tcl)
            && atoms[at].arg == (RefAtomArg::Raw { size: false })
        {
            return Err(fatal(format!("--format={} cannot be used with --python, --shell, --tcl", text)));
        }
        if let Some(color) = atoms[at].name.strip_prefix("color:") {
            need_reset = color != "reset";
        }
        pieces.push((None, Some(at)));
        cp = ep + 1;
    }
    if cp < format.len() {
        pieces.push((Some(format[cp..].to_string()), None));
    }
    if !use_color {
        need_reset = false;
    }

    let mut sorts = Vec::new();
    let keys: Vec<String> = if opts.sorts.is_empty() { vec!["refname".to_string()] } else { opts.sorts.clone() };
    for key in &keys {
        let (reverse, rest) = match key.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, key.as_str()),
        };
        let (version, rest) = match rest.strip_prefix("version:").or_else(|| rest.strip_prefix("v:")) {
            Some(rest) => (true, rest),
            None => (false, rest),
        };
        let at = parse_ref_atom(atoms, rest, use_color).map_err(fatal)?;
        sorts.insert(0, (at, reverse, version));
    }
    Ok((pieces, need_reset, sorts))
}

fn find_next_atom(format: &str, from: usize) -> Option<usize> {
    let bytes = format.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            match bytes.get(i + 1) {
                Some(b'(') => return Some(i),
                Some(b'%') => i += 1,
                _ => {}
            }
        }
        i += 1;
    }
    None
}

fn atom_table(name: &str) -> Option<(RefAtomKind, u8, bool)> {
    use RefAtomKind::*;
    Some(match name {
        "refname" => (Refname, 0, false),
        "objecttype" => (ObjectType, 2, false),
        "objectsize" => (ObjectSize, 2, true),
        "objectname" => (ObjectName, 2, false),
        "deltabase" => (DeltaBase, 2, false),
        "tree" => (Tree, 1, false),
        "parent" => (Parent, 1, false),
        "numparent" => (NumParent, 1, true),
        "object" => (Object, 1, false),
        "type" => (Type, 1, false),
        "tag" => (Tag, 1, false),
        "author" => (Author, 1, false),
        "authorname" => (AuthorName, 1, false),
        "authoremail" => (AuthorEmail, 1, false),
        "authordate" => (AuthorDate, 1, true),
        "committer" => (Committer, 1, false),
        "committername" => (CommitterName, 1, false),
        "committeremail" => (CommitterEmail, 1, false),
        "committerdate" => (CommitterDate, 1, true),
        "tagger" => (Tagger, 1, false),
        "taggername" => (TaggerName, 1, false),
        "taggeremail" => (TaggerEmail, 1, false),
        "taggerdate" => (TaggerDate, 1, true),
        "creator" => (Creator, 1, false),
        "creatordate" => (CreatorDate, 1, true),
        "describe" => (Describe, 1, false),
        "subject" => (Subject, 1, false),
        "body" => (Body, 1, false),
        "trailers" => (Trailers, 1, false),
        "contents" => (Contents, 1, false),
        "signature" => (Signature, 1, false),
        "raw" => (Raw, 1, false),
        "upstream" => (Upstream, 0, false),
        "push" => (Push, 0, false),
        "symref" => (Symref, 0, false),
        "flag" => (Flag, 0, false),
        "HEAD" => (Head, 0, false),
        "color" => (Color, 0, false),
        "worktreepath" => (WorktreePath, 0, false),
        "align" => (Align, 0, false),
        "end" => (End, 0, false),
        "if" => (If, 0, false),
        "then" => (Then, 0, false),
        "else" => (Else, 0, false),
        "rest" => (Rest, 0, false),
        "ahead-behind" => (AheadBehind, 2, false),
        _ => return None,
    })
}

fn atom_display_name(name: &str) -> &str {
    name.split(':').next().unwrap_or(name)
}

fn err_no_arg(name: &str) -> String {
    format!("%({}) does not take arguments", atom_display_name(name))
}

fn err_bad_arg(name: &str, arg: &str) -> String {
    format!("unrecognized %({}) argument: {}", atom_display_name(name), arg)
}

/// Parses one `%(...)` atom (the text between the parentheses) like git's
/// `parse_ref_filter_atom`, adding it to `atoms` unless the same text was already parsed, and
/// returns its index. Errors are git's messages (without `fatal: `).
// Used by `for-each-ref` via `prepare_ref_format`.
pub fn parse_ref_atom(atoms: &mut Vec<RefFilterAtom>, text: &str, use_color: bool) -> Result<usize, String> {
    let deref = text.starts_with('*');
    let sp = if deref { &text[1..] } else { text };
    if sp.is_empty() {
        return Err(format!("malformed field name: {}", text));
    }
    let (base, arg) = match sp.find(':') {
        Some(pos) => (&sp[..pos], Some(&sp[pos + 1..])),
        None => (sp, None),
    };
    if let Some(i) = atoms.iter().position(|a| a.name == text) {
        return Ok(i);
    }
    let (kind, source, numeric) = atom_table(base).ok_or_else(|| format!("unknown field name: {}", text))?;
    let arg = arg.filter(|a| !a.is_empty());
    let mut numeric = numeric;
    let name = text;

    use RefAtomKind as K;
    let parsed = match kind {
        K::Refname | K::Symref => RefAtomArg::Refname(parse_refname_show(arg, name)?),
        K::Upstream | K::Push => parse_remote_ref_arg(kind == K::Push, arg, name)?,
        K::ObjectType | K::DeltaBase | K::Rest | K::Head | K::Body => {
            if arg.is_some() {
                return Err(err_no_arg(match kind {
                    K::ObjectType => "objecttype",
                    K::DeltaBase => "deltabase",
                    K::Rest => "rest",
                    K::Head => "HEAD",
                    _ => "body",
                }));
            }
            match kind {
                K::Head => RefAtomArg::Head(resolve_head_ref()),
                K::Body => RefAtomArg::Contents(RefContentsPart::BodyWithSignature),
                _ => RefAtomArg::None,
            }
        }
        K::ObjectSize => match arg {
            None => RefAtomArg::ObjectSize { disk: false },
            Some("disk") => RefAtomArg::ObjectSize { disk: true },
            Some(other) => return Err(err_bad_arg("objectsize", other)),
        },
        K::ObjectName | K::Tree | K::Parent => RefAtomArg::Oid(match arg {
            None => None,
            Some("short") => Some(7),
            Some(other) => match other.strip_prefix("short=") {
                Some(n) => match parse_c_ulong(n) {
                    Some(n) if n > 0 => Some((n as usize).max(4)),
                    _ => return Err(format!("positive value expected '{}' in %({})", n, name)),
                },
                None => return Err(err_bad_arg(name, other)),
            },
        }),
        K::AuthorName | K::CommitterName | K::TaggerName => match arg {
            None | Some("mailmap") => RefAtomArg::None,
            Some(other) => return Err(err_bad_arg(name, other)),
        },
        K::AuthorEmail | K::CommitterEmail | K::TaggerEmail => parse_email_arg(arg, name)?,
        K::Subject => match arg {
            None => RefAtomArg::Contents(RefContentsPart::Subject),
            Some("sanitize") => RefAtomArg::Contents(RefContentsPart::SubjectSanitized),
            Some(other) => return Err(err_bad_arg("subject", other)),
        },
        K::Trailers => RefAtomArg::Contents(RefContentsPart::Trailers(parse_trailer_options(arg)?)),
        K::Contents => RefAtomArg::Contents(match arg {
            None => RefContentsPart::Bare,
            Some("body") => RefContentsPart::Body,
            Some("size") => {
                numeric = true;
                RefContentsPart::Size
            }
            Some("signature") => RefContentsPart::Signature,
            Some("subject") => RefContentsPart::Subject,
            Some("trailers") => RefContentsPart::Trailers(parse_trailer_options(None)?),
            Some(other) => {
                if let Some(opts) = other.strip_prefix("trailers:") {
                    RefContentsPart::Trailers(parse_trailer_options(Some(opts))?)
                } else if let Some(n) = other.strip_prefix("lines=") {
                    match parse_c_ulong(n) {
                        Some(n) => RefContentsPart::Lines(n as usize),
                        None => return Err(format!("positive value expected contents:lines={}", n)),
                    }
                } else {
                    return Err(err_bad_arg("contents", other));
                }
            }
        }),
        K::Signature => match arg {
            None => RefAtomArg::Signature(String::new()),
            Some(o @ ("signer" | "grade" | "key" | "fingerprint" | "primarykeyfingerprint" | "trustlevel")) => RefAtomArg::Signature(o.to_string()),
            Some(other) => return Err(err_bad_arg("signature", other)),
        },
        K::Raw => match arg {
            None => RefAtomArg::Raw { size: false },
            Some("size") => {
                numeric = true;
                RefAtomArg::Raw { size: true }
            }
            Some(other) => return Err(err_bad_arg("raw", other)),
        },
        K::Describe => parse_describe_arg(arg)?,
        K::Color => {
            let Some(value) = arg else { return Err("expected format: %(color:<color>)".to_string()) };
            let escape = parse_color(value).map_err(|_| {
                eprintln!("error: invalid color value: {}", value);
                format!("unrecognized color: %(color:{})", value)
            })?;
            RefAtomArg::Color(if use_color { escape } else { String::new() })
        }
        K::Align => parse_align_arg(arg)?,
        K::If => match arg {
            None => RefAtomArg::If(None),
            Some(a) => {
                if let Some(s) = a.strip_prefix("equals=") {
                    RefAtomArg::If(Some((true, s.to_string())))
                } else if let Some(s) = a.strip_prefix("notequals=") {
                    RefAtomArg::If(Some((false, s.to_string())))
                } else {
                    return Err(err_bad_arg("if", a));
                }
            }
        },
        K::AheadBehind => {
            let Some(base) = arg else { return Err("expected format: %(ahead-behind:<committish>)".to_string()) };
            let commit = resolve_rev(base).ok().and_then(|h| peel_to_commit(&h)).ok_or_else(|| format!("failed to find '{}'", base))?;
            RefAtomArg::AheadBehind(commit)
        }
        _ => RefAtomArg::None,
    };

    atoms.push(RefFilterAtom { kind, name: name.to_string(), deref, numeric, needs_content: source == 1, arg: parsed });
    Ok(atoms.len() - 1)
}

fn parse_c_ulong(text: &str) -> Option<u64> {
    let t = text.strip_prefix('+').unwrap_or(text);
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok().filter(|n: &u64| *n <= u32::MAX as u64)
}

fn parse_c_int(text: &str) -> Option<i64> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: i64 = digits.parse().ok()?;
    let n = if negative { -n } else { n };
    (i32::MIN as i64..=i32::MAX as i64).contains(&n).then_some(n)
}

fn parse_refname_show(arg: Option<&str>, name: &str) -> Result<RefnameShow, String> {
    match arg {
        None => Ok(RefnameShow::Full),
        Some("short") => Ok(RefnameShow::Short),
        Some(a) => {
            if let Some(n) = a.strip_prefix("lstrip=").or_else(|| a.strip_prefix("strip=")) {
                parse_c_int(n).map(RefnameShow::Lstrip).ok_or_else(|| format!("Integer value expected refname:lstrip={}", n))
            } else if let Some(n) = a.strip_prefix("rstrip=") {
                parse_c_int(n).map(RefnameShow::Rstrip).ok_or_else(|| format!("Integer value expected refname:rstrip={}", n))
            } else {
                Err(err_bad_arg(name, a))
            }
        }
    }
}

fn parse_remote_ref_arg(push: bool, arg: Option<&str>, name: &str) -> Result<RefAtomArg, String> {
    let mut show = RemoteRefShow::Ref;
    let mut refname = RefnameShow::Full;
    let mut nobracket = false;
    let mut push_remote = false;
    if let Some(arg) = arg {
        for param in arg.split(',') {
            match param {
                "track" => show = RemoteRefShow::Track,
                "trackshort" => show = RemoteRefShow::TrackShort,
                "nobracket" => nobracket = true,
                "remotename" => {
                    show = RemoteRefShow::RemoteName;
                    push_remote = true;
                }
                "remoteref" => {
                    show = RemoteRefShow::RemoteRef;
                    push_remote = true;
                }
                _ => {
                    show = RemoteRefShow::Ref;
                    refname = parse_refname_show(Some(arg), name)?;
                }
            }
        }
    }
    Ok(RefAtomArg::RemoteRef { show, refname, nobracket, push, push_remote })
}

fn parse_email_arg(arg: Option<&str>, name: &str) -> Result<RefAtomArg, String> {
    let (mut trim, mut localpart) = (false, false);
    let Some(mut rest) = arg else { return Ok(RefAtomArg::Email { trim, localpart }) };
    loop {
        if let Some(r) = rest.strip_prefix("trim") {
            trim = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("localpart") {
            localpart = true;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("mailmap") {
            rest = r;
        } else {
            return Err(err_bad_arg(name, rest));
        }
        if rest.is_empty() {
            break;
        }
        match rest.strip_prefix(',') {
            Some(r) => rest = r,
            None => return Err(err_bad_arg(name, rest)),
        }
    }
    Ok(RefAtomArg::Email { trim, localpart })
}

/// git's `match_atom_arg_value`: `candidate` alone, or `candidate=value`, up to the next `,`.
/// Returns `(value, rest)`.
fn match_atom_arg<'a>(to_parse: &'a str, candidate: &str) -> Option<(Option<&'a str>, &'a str)> {
    let rest = to_parse.strip_prefix(candidate)?;
    let (value, after) = if let Some(v) = rest.strip_prefix('=') {
        let end = v.find(',').unwrap_or(v.len());
        (Some(&v[..end]), &v[end..])
    } else if rest.is_empty() || rest.starts_with(',') {
        (None, rest)
    } else {
        return None;
    };
    Some((value, after.strip_prefix(',').unwrap_or(after)))
}

pub(super) fn parse_maybe_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "" | "false" | "no" | "off" => Some(false),
        "true" | "yes" | "on" => Some(true),
        other => parse_c_int(other).map(|n| n != 0),
    }
}

fn parse_describe_arg(arg: Option<&str>) -> Result<RefAtomArg, String> {
    let (mut tags, mut abbrev, mut matches, mut excludes) = (None, None, Vec::new(), Vec::new());
    let mut rest = arg.unwrap_or("");
    while !rest.is_empty() {
        let bad = rest;
        if let Some((value, after)) = match_atom_arg(rest, "tags").and_then(|(v, a)| match v {
            None => Some((true, a)),
            Some(v) => parse_maybe_bool(v).map(|b| (b, a)),
        }) {
            tags = Some(value);
            rest = after;
        } else if let Some((value, after)) = match_atom_arg(rest, "abbrev") {
            let value = value.unwrap_or("");
            if value.is_empty() {
                return Err("argument expected for describe:abbrev".to_string());
            }
            let full = &bad["abbrev=".len()..];
            let digits_end = value.trim_start_matches(['-', '+']).find(|c: char| !c.is_ascii_digit()).map(|p| p + value.len() - value.trim_start_matches(['-', '+']).len());
            let parsed: Option<i64> = value.parse().ok();
            if value.starts_with('-') && value[1..].bytes().next().is_some_and(|b| b.is_ascii_digit()) && parsed.is_none_or(|n| n < 0) {
                return Err(format!("positive value expected describe:abbrev={}", full));
            }
            if digits_end.is_some() || parsed.is_none() {
                return Err(format!("cannot fully parse describe:abbrev={}", full));
            }
            let n = parsed.unwrap_or(0).max(0) as usize;
            abbrev = Some(if n == 0 { 0 } else { n.clamp(4, 40) });
            rest = after;
        } else if let Some((value, after)) = match_atom_arg(rest, "match") {
            match value.filter(|v| !v.is_empty()) {
                Some(v) => matches.push(v.to_string()),
                None => return Err("value expected describe:match=".to_string()),
            }
            rest = after;
        } else if let Some((value, after)) = match_atom_arg(rest, "exclude") {
            match value.filter(|v| !v.is_empty()) {
                Some(v) => excludes.push(v.to_string()),
                None => return Err("value expected describe:exclude=".to_string()),
            }
            rest = after;
        } else {
            return Err(err_bad_arg("describe", bad));
        }
    }
    Ok(RefAtomArg::Describe { tags, abbrev, matches, excludes })
}

fn parse_align_arg(arg: Option<&str>) -> Result<RefAtomArg, String> {
    let Some(arg) = arg else { return Err("expected format: %(align:<width>,<position>)".to_string()) };
    let mut position = RefAlign::Left;
    let mut width: Option<u64> = None;
    let position_of = |s: &str| match s {
        "right" => Some(RefAlign::Right),
        "middle" => Some(RefAlign::Middle),
        "left" => Some(RefAlign::Left),
        _ => None,
    };
    for s in arg.split(',') {
        if let Some(p) = s.strip_prefix("position=") {
            position = position_of(p).ok_or_else(|| format!("unrecognized position:{}", p))?;
        } else if let Some(w) = s.strip_prefix("width=") {
            width = Some(parse_c_ulong(w).ok_or_else(|| format!("unrecognized width:{}", w))?);
        } else if let Some(w) = parse_c_ulong(s) {
            width = Some(w);
        } else if let Some(p) = position_of(s) {
            position = p;
        } else {
            return Err(format!("unrecognized %(align) argument: {}", s));
        }
    }
    match width {
        Some(w) if w != u32::MAX as u64 => Ok(RefAtomArg::Align { position, width: w as usize }),
        _ => Err("positive width expected with the %(align) atom".to_string()),
    }
}

/// Parses `%(trailers:...)` options like git's `format_set_trailers_options`.
// Used by `for-each-ref` (`%(trailers)`, `%(contents:trailers)`).
pub fn parse_trailer_options(arg: Option<&str>) -> Result<TrailerFormatOptions, String> {
    let mut opts = TrailerFormatOptions::default();
    let Some(arg) = arg else { return Ok(opts) };
    let full = format!("{})", arg);
    let mut rest = full.as_str();
    let placeholder = |to_parse: &str, candidate: &str| -> Option<(Option<String>, usize)> {
        let p = to_parse.strip_prefix(candidate)?;
        let mut consumed = candidate.len();
        let mut value = None;
        let after = if let Some(v) = p.strip_prefix('=') {
            let end = v.find([',', ')']).unwrap_or(v.len());
            value = Some(v[..end].to_string());
            consumed += 1 + end;
            &v[end..]
        } else {
            if !p.starts_with([',', ')']) {
                return None;
            }
            p
        };
        if after.starts_with(',') {
            Some((value, consumed + 1))
        } else if after.starts_with(')') {
            Some((value, consumed))
        } else {
            None
        }
    };
    let bool_arg = |to_parse: &str, candidate: &str| -> Option<(Option<bool>, usize)> {
        let (value, used) = placeholder(to_parse, candidate)?;
        match value {
            None => Some((Some(true), used)),
            Some(v) => Some((parse_maybe_bool(&v), used)),
        }
    };
    let invalid = |rest: &str| format!("unknown %(trailers) argument: {}", &rest[..rest.find([',', ')']).unwrap_or(rest.len())]);
    while !rest.starts_with(')') {
        if let Some((value, used)) = placeholder(rest, "key") {
            let Some(mut key) = value else { return Err("expected %(trailers:key=<value>)".to_string()) };
            if key.ends_with(':') {
                key.pop();
            }
            opts.keys.push(key);
            opts.only = true;
            rest = &rest[used..];
        } else if let Some((value, used)) = placeholder(rest, "separator") {
            opts.separator = Some(expand_literal(value.as_deref().unwrap_or("")));
            rest = &rest[used..];
        } else if let Some((value, used)) = placeholder(rest, "key_value_separator") {
            opts.key_value_separator = Some(expand_literal(value.as_deref().unwrap_or("")));
            rest = &rest[used..];
        } else if let Some((v, used)) = bool_arg(rest, "only") {
            rest = &rest[used..];
            opts.only = v.ok_or_else(|| invalid(rest))?;
        } else if let Some((v, used)) = bool_arg(rest, "unfold") {
            rest = &rest[used..];
            opts.unfold = v.ok_or_else(|| invalid(rest))?;
        } else if let Some((v, used)) = bool_arg(rest, "keyonly") {
            rest = &rest[used..];
            opts.key_only = v.ok_or_else(|| invalid(rest))?;
        } else if let Some((v, used)) = bool_arg(rest, "valueonly") {
            rest = &rest[used..];
            opts.value_only = v.ok_or_else(|| invalid(rest))?;
        } else {
            return Err(invalid(rest));
        }
    }
    Ok(opts)
}

pub(super) fn hex_pair(bytes: &[u8]) -> Option<u8> {
    let h = |c: u8| (c as char).to_digit(16);
    Some((h(*bytes.first()?)? * 16 + h(*bytes.get(1)?)?) as u8)
}

fn expand_literal(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        i += 1;
        match bytes.get(i) {
            Some(b'%') => {
                out.push(b'%');
                i += 1;
            }
            Some(b'n') => {
                out.push(b'\n');
                i += 1;
            }
            Some(b'x') if hex_pair(&bytes[i + 1..]).is_some() => {
                out.push(hex_pair(&bytes[i + 1..]).unwrap_or(0));
                i += 3;
            }
            _ => out.push(b'%'),
        }
    }
    out
}
