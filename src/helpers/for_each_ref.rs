use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::IsTerminal;
use std::path::Path;
use crate::helpers::bisect::collect_ancestors;
use crate::helpers::check_ref_format::check_refname_format;
use crate::helpers::cherry::abbreviate_hash_min;
use crate::helpers::config::{get_config_value, global_config_path, normalize_key, read_entries_from, LOCAL_CONFIG_PATH};
use crate::helpers::format_patch::civil_from_days;
use crate::helpers::read_object;
use crate::helpers::rev_parse::resolve_rev;
use crate::helpers::tag::describe_commit_matching;
use crate::helpers::wildmatch::{wildmatch, WM_CASEFOLD, WM_PATHNAME};
use crate::objects::{
    ForEachRefExit, ForEachRefOptions, RefAlign, RefArrayItem, RefAtomArg, RefAtomKind, RefAtomValue, RefContentsPart, RefDateKind,
    RefDateMode, RefFilterAtom, RefFormatPiece, RefQuoteStyle, RefnameShow, RemoteRefShow, TrailerFormatOptions,
};

/// git's `--format` default for `for-each-ref`.
pub const FOR_EACH_REF_DEFAULT_FORMAT: &str = "%(objectname) %(objecttype)\t%(refname)";

/// git's `for-each-ref -h` text, also printed after unknown options (exit 129).
pub const FOR_EACH_REF_USAGE: &str = "usage: rgit for-each-ref [<options>] [<pattern>]
   or: rgit for-each-ref [--points-at <object>]
   or: rgit for-each-ref [--merged [<commit>]] [--no-merged [<commit>]]
   or: rgit for-each-ref [--contains [<commit>]] [--no-contains [<commit>]]

    -s, --[no-]shell      quote placeholders suitably for shells
    -p, --[no-]perl       quote placeholders suitably for perl
    --[no-]python         quote placeholders suitably for python
    --[no-]tcl            quote placeholders suitably for Tcl
    --[no-]omit-empty     do not output a newline after empty formatted refs

    --[no-]count <n>      show only <n> matched refs
    --[no-]format <format>
                          format to use for the output
    --[no-]color[=<when>] respect format colors
    --[no-]exclude <pattern>
                          exclude refs which match pattern
    --[no-]sort <key>     field name to sort on
    --[no-]points-at <object>
                          print only refs which points at the given object
    --merged <commit>     print only refs that are merged
    --no-merged <commit>  print only refs that are not merged
    --contains <commit>   print only refs which contain the commit
    --no-contains <commit>
                          print only refs which don't contain the commit
    --[no-]ignore-case    sorting and filtering are case insensitive
    --[no-]stdin          read reference patterns from stdin
";

const LONG_OPTIONS: &[(&str, bool)] = &[
    ("shell", true),
    ("perl", true),
    ("python", true),
    ("tcl", true),
    ("omit-empty", true),
    ("count", true),
    ("format", true),
    ("color", true),
    ("exclude", true),
    ("sort", true),
    ("points-at", true),
    ("merged", false),
    ("no-merged", false),
    ("contains", false),
    ("no-contains", false),
    ("ignore-case", true),
    ("stdin", true),
];

const PER_WORKTREE_REFS: &[&str] = &["refs/bisect", "refs/worktree", "refs/rewritten"];
const SHORTEN_RULES: &[(&str, &str)] =
    &[("", ""), ("refs/", ""), ("refs/tags/", ""), ("refs/heads/", ""), ("refs/remotes/", ""), ("refs/remotes/", "/HEAD")];
const COLOR_RESET: &str = "\x1b[m";

fn exit_error(code: i32, message: String) -> ForEachRefExit {
    ForEachRefExit { code, stdout: String::new(), stderr: message }
}

fn usage_error(message: String) -> ForEachRefExit {
    exit_error(129, format!("{}\n{}", message, FOR_EACH_REF_USAGE))
}

fn fatal(message: String) -> ForEachRefExit {
    exit_error(128, format!("fatal: {}", message))
}

/// Parses `for-each-ref` arguments like git's option parser: unique prefixes of long options,
/// `--no-<option>` negations, bundled `-s`/`-p`, values as `--opt=v` or `--opt v`, and
/// `--merged`/`--no-merged`/`--contains`/`--no-contains` taking HEAD when they are the last
/// argument. Revisions are resolved while parsing, so their errors come first, as in git.
// Used by `for-each-ref`.
pub fn parse_for_each_ref_args(args: &[String]) -> Result<ForEachRefOptions, ForEachRefExit> {
    let mut opts = ForEachRefOptions { format: FOR_EACH_REF_DEFAULT_FORMAT.to_string(), ..Default::default() };
    let mut quote_bits = 0u8;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            opts.patterns.extend(args[i + 1..].iter().cloned());
            break;
        }
        if let Some(long) = arg.strip_prefix("--") {
            if long == "help" {
                return Err(ForEachRefExit { code: 129, stdout: format!("{}\n", FOR_EACH_REF_USAGE), stderr: String::new() });
            }
            let (index, unset, value) = resolve_long_option(long)?;
            i = apply_long_option(&mut opts, &mut quote_bits, LONG_OPTIONS[index].0, unset, value, args, i)?;
        } else if arg.len() > 1 && arg.starts_with('-') {
            for c in arg[1..].chars() {
                match c {
                    's' => quote_bits |= 1,
                    'p' => quote_bits |= 2,
                    'h' => return Err(ForEachRefExit { code: 129, stdout: format!("{}\n", FOR_EACH_REF_USAGE), stderr: String::new() }),
                    other => return Err(usage_error(format!("error: unknown switch `{}'", other))),
                }
            }
        } else {
            opts.patterns.push(arg.to_string());
        }
        i += 1;
    }

    if opts.count < 0 {
        return Err(usage_error(format!("error: invalid --count argument: `{}'", opts.count)));
    }
    if quote_bits.count_ones() > 1 {
        return Err(usage_error("error: more than one quoting style?".to_string()));
    }
    opts.quote = match quote_bits {
        1 => RefQuoteStyle::Shell,
        2 => RefQuoteStyle::Perl,
        4 => RefQuoteStyle::Python,
        8 => RefQuoteStyle::Tcl,
        _ => RefQuoteStyle::None,
    };
    opts.quote_bits = quote_bits;
    Ok(opts)
}

fn resolve_long_option(arg: &str) -> Result<(usize, bool, Option<String>), ForEachRefExit> {
    let arg_end = arg.find('=').unwrap_or(arg.len());
    let head = &arg[..arg_end];
    let mut abbrev: Option<(usize, bool)> = None;
    let mut ambiguous: Option<(usize, bool)> = None;

    'options: for (index, &(full_name, negatable)) in LONG_OPTIONS.iter().enumerate() {
        let mut long_name = full_name;
        let mut opt_unset = false;
        loop {
            let mut flags_unset = false;
            let rest = match arg.strip_prefix(long_name) {
                Some(rest) => rest,
                None => {
                    let abbreviated = if long_name.starts_with(head) {
                        true
                    } else if !negatable {
                        continue 'options;
                    } else if "no-".starts_with(arg) {
                        flags_unset = true;
                        true
                    } else if !arg.starts_with("no-") {
                        if let Some(stripped) = long_name.strip_prefix("no-") {
                            long_name = stripped;
                            opt_unset = true;
                            continue;
                        }
                        continue 'options;
                    } else {
                        flags_unset = true;
                        match arg[3..].strip_prefix(long_name) {
                            Some(rest) => {
                                if !rest.is_empty() && !rest.starts_with('=') {
                                    continue 'options;
                                }
                                let value = rest.strip_prefix('=').map(|v| v.to_string());
                                return Ok((index, flags_unset ^ opt_unset, value));
                            }
                            None if long_name.starts_with(&arg[3..]) => true,
                            None => continue 'options,
                        }
                    };
                    if abbreviated {
                        if abbrev.is_some() {
                            ambiguous = abbrev;
                        }
                        abbrev = Some((index, flags_unset ^ opt_unset));
                    }
                    continue 'options;
                }
            };
            if !rest.is_empty() && !rest.starts_with('=') {
                continue 'options;
            }
            let value = rest.strip_prefix('=').map(|v| v.to_string());
            return Ok((index, flags_unset ^ opt_unset, value));
        }
    }

    if let (Some((amb, amb_unset)), Some((abb, abb_unset))) = (ambiguous, abbrev) {
        return Err(ForEachRefExit {
            code: 129,
            stdout: format!("{}\n", FOR_EACH_REF_USAGE),
            stderr: format!(
                "error: ambiguous option: {} (could be --{}{} or --{}{})",
                arg,
                if amb_unset { "no-" } else { "" },
                LONG_OPTIONS[amb].0,
                if abb_unset { "no-" } else { "" },
                LONG_OPTIONS[abb].0
            ),
        });
    }
    match abbrev {
        Some((index, unset)) => Ok((index, unset, if arg_end < arg.len() { Some(arg[arg_end + 1..].to_string()) } else { None })),
        None => Err(usage_error(format!("error: unknown option `{}'", arg))),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_long_option(
    opts: &mut ForEachRefOptions,
    quote_bits: &mut u8,
    name: &str,
    unset: bool,
    value: Option<String>,
    args: &[String],
    mut i: usize,
) -> Result<usize, ForEachRefExit> {
    let shown = if unset { format!("no-{}", name) } else { name.to_string() };
    let no_value = |value: &Option<String>| -> Result<(), ForEachRefExit> {
        match value {
            Some(_) => Err(exit_error(129, format!("error: option `{}' takes no value", shown))),
            None => Ok(()),
        }
    };
    let take_value = |value: Option<String>, i: &mut usize, last_default: bool| -> Result<String, ForEachRefExit> {
        if let Some(v) = value {
            return Ok(v);
        }
        if last_default && *i + 1 == args.len() {
            return Ok("HEAD".to_string());
        }
        if *i + 1 < args.len() {
            *i += 1;
            return Ok(args[*i].clone());
        }
        Err(exit_error(129, format!("error: option `{}' requires a value", shown)))
    };

    let bit = match name {
        "shell" => 1,
        "perl" => 2,
        "python" => 4,
        "tcl" => 8,
        _ => 0,
    };
    if bit != 0 {
        no_value(&value)?;
        if unset {
            *quote_bits &= !bit;
        } else {
            *quote_bits |= bit;
        }
        return Ok(i);
    }

    match name {
        "omit-empty" | "ignore-case" | "stdin" => {
            no_value(&value)?;
            let flag = match name {
                "omit-empty" => &mut opts.omit_empty,
                "ignore-case" => &mut opts.ignore_case,
                _ => &mut opts.stdin,
            };
            *flag = !unset;
        }
        "count" => {
            if unset {
                no_value(&value)?;
                opts.count = 0;
            } else {
                let v = take_value(value, &mut i, false)?;
                opts.count = parse_c_long(&v).ok_or_else(|| exit_error(129, "error: option `count' expects a numerical value".to_string()))?;
            }
        }
        "format" => {
            if unset {
                no_value(&value)?;
                opts.format = FOR_EACH_REF_DEFAULT_FORMAT.to_string();
            } else {
                opts.format = take_value(value, &mut i, false)?;
            }
        }
        "color" => {
            if unset && value.is_some() {
                no_value(&value)?;
            }
            let when = value.unwrap_or_else(|| if unset { "never".to_string() } else { "always".to_string() });
            opts.color = match when.to_ascii_lowercase().as_str() {
                "never" => Some(false),
                "always" => Some(true),
                "auto" => None,
                _ => return Err(exit_error(129, "error: option `color' expects \"always\", \"auto\", or \"never\"".to_string())),
            };
        }
        "exclude" | "sort" | "points-at" => {
            if unset {
                no_value(&value)?;
                match name {
                    "exclude" => opts.excludes.clear(),
                    "sort" => opts.sorts.clear(),
                    _ => opts.points_at.clear(),
                }
            } else {
                let v = take_value(value, &mut i, false)?;
                match name {
                    "exclude" => opts.excludes.push(v),
                    "sort" => opts.sorts.push(v),
                    _ => {
                        let oid = resolve_rev(&v).map_err(|_| exit_error(129, format!("error: malformed object name '{}'", v)))?;
                        opts.points_at.push(oid);
                    }
                }
            }
        }
        "merged" | "no-merged" => {
            let v = take_value(value, &mut i, true)?;
            let oid = resolve_rev(&v).map_err(|_| fatal(format!("malformed object name {}", v)))?;
            let commit = peel_to_commit_reporting(&oid).map_err(|e| exit_error(129, format!("{}\nerror: option `{}' must point to a commit", e, name)))?;
            if name == "merged" {
                opts.merged.push(commit);
            } else {
                opts.no_merged.push(commit);
            }
        }
        _ => {
            let v = take_value(value, &mut i, true)?;
            let oid = resolve_rev(&v).map_err(|_| exit_error(129, format!("error: malformed object name {}", v)))?;
            let commit = peel_to_commit_reporting(&oid).map_err(|e| exit_error(129, format!("{}\nerror: no such commit {}", e, v)))?;
            if name == "contains" {
                opts.contains.push(commit);
            } else {
                opts.no_contains.push(commit);
            }
        }
    }
    Ok(i)
}

fn parse_c_long(text: &str) -> Option<i64> {
    let t = text.trim_start();
    let (negative, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if text.is_empty() {
        return Some(0);
    }
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: i64 = digits.parse().ok()?;
    Some(if negative { -n } else { n })
}

/// Peels `oid` through tags to a commit, or explains (like git's
/// `lookup_commit_reference`) which object was in the way.
fn peel_to_commit_reporting(oid: &str) -> Result<String, String> {
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

fn peel_to_commit(oid: &str) -> Option<String> {
    peel_to_commit_reporting(oid).ok()
}

fn tag_target(content: &[u8]) -> Option<String> {
    header_value(content, "object").filter(|h| h.len() == 40)
}

fn header_value(content: &[u8], key: &str) -> Option<String> {
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

fn parse_maybe_bool(value: &str) -> Option<bool> {
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

fn hex_pair(bytes: &[u8]) -> Option<u8> {
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

/// The ref HEAD names, followed through symbolic refs, when it exists (git's
/// `resolve_refdup("HEAD")`): `refs/heads/<branch>`, `HEAD` when detached, `None` when unborn.
fn resolve_head_ref() -> Option<String> {
    resolve_ref_chain("HEAD").map(|(name, _, _)| name)
}

/// Reads one ref without following symbolic refs: `Some(Ok(hash))`, `Some(Err(target))` for a
/// `ref: ` symref, along with whether it came from `packed-refs`. `None` if it doesn't exist or
/// is unreadable.
fn read_raw_ref(name: &str) -> Option<(Result<String, String>, bool)> {
    let path = crate::repo::path(format!(".git/{}", name));
    if Path::new(&path).is_file() {
        let content = fs::read_to_string(&path).ok()?;
        return parse_loose_ref(&content).map(|v| (v, false));
    }
    if Path::new(&path).is_dir() {
        return None;
    }
    packed_refs().get(name).map(|h| (Ok(h.clone()), true))
}

fn parse_loose_ref(content: &str) -> Option<Result<String, String>> {
    if let Some(target) = content.strip_prefix("ref:") {
        return Some(Err(target.trim().to_string()));
    }
    let hash = content.get(..40)?;
    let after = content[40..].chars().next();
    (hash.chars().all(|c| c.is_ascii_hexdigit()) && after.is_none_or(|c| c.is_whitespace())).then(|| Ok(hash.to_ascii_lowercase()))
}

fn packed_refs() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Ok(packed) = fs::read_to_string(crate::repo::path(".git/packed-refs")) {
        for line in packed.lines() {
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some((hash, name)) = line.split_once(' ') {
                if hash.len() == 40 {
                    out.insert(name.to_string(), hash.to_string());
                }
            }
        }
    }
    out
}

/// Follows a ref through symbolic refs (at most 5 levels): `(final name, hash, any part packed)`.
fn resolve_ref_chain(name: &str) -> Option<(String, String, bool)> {
    let mut current = name.to_string();
    let mut packed = false;
    for _ in 0..6 {
        let (value, from_packed) = read_raw_ref(&current)?;
        packed |= from_packed;
        match value {
            Ok(hash) => return Some((current, hash, packed)),
            Err(target) => {
                if !check_refname_format(&target, true, false) && target != "HEAD" {
                    return None;
                }
                current = target;
            }
        }
    }
    None
}

fn ref_exists(name: &str) -> bool {
    resolve_ref_chain(name).is_some()
}

/// Lists every ref under `refs/` (loose and packed, per-worktree refs from the current
/// worktree) in name order, like git's ref iteration (only those under `prefixes`, when
/// given): refs with bad names or unreadable
/// contents are skipped with git's warnings, dangling symbolic refs silently.
// Used by `for-each-ref`.
pub fn collect_ref_items(prefixes: Option<&[String]>) -> Vec<RefArrayItem> {
    let linked = crate::repo::is_linked_worktree();
    let mut loose: BTreeMap<String, String> = BTreeMap::new();
    walk_loose_refs(&crate::repo::path(".git/refs"), "refs", linked, &mut loose);
    if linked {
        for prefix in PER_WORKTREE_REFS {
            walk_loose_refs(&crate::repo::path(format!(".git/{}", prefix)), prefix, false, &mut loose);
        }
    }
    let mut names: BTreeMap<String, bool> = BTreeMap::new();
    for name in loose.keys() {
        names.insert(name.clone(), false);
    }
    let packed_map = packed_refs();
    for name in packed_map.keys() {
        if name.starts_with("refs/") {
            names.entry(name.clone()).or_insert(true);
        }
    }

    let mut items = Vec::new();
    for (name, packed_only) in names {
        if prefixes.is_some_and(|ps| !ps.iter().any(|p| name.starts_with(p.as_str()))) {
            continue;
        }
        if !check_refname_format(&name, true, false) {
            eprintln!("warning: ignoring ref with broken name {}", name);
            continue;
        }
        let (oid, symref, packed) = if packed_only {
            match packed_map.get(&name) {
                Some(h) => (h.to_ascii_lowercase(), false, true),
                None => continue,
            }
        } else {
            match parse_loose_ref(&loose[&name]) {
                Some(Ok(hash)) => (hash, false, false),
                Some(Err(_)) => match resolve_ref_chain(&name) {
                    Some((_, hash, packed)) => (hash, true, packed),
                    None => continue,
                },
                None => {
                    eprintln!("warning: ignoring broken ref {}", name);
                    continue;
                }
            }
        };
        if oid.bytes().all(|b| b == b'0') {
            if !symref {
                eprintln!("warning: ignoring broken ref {}", name);
            }
            continue;
        }
        items.push(RefArrayItem { refname: name, oid, symref, packed });
    }
    items
}

fn walk_loose_refs(dir: &Path, prefix: &str, skip_per_worktree: bool, out: &mut BTreeMap<String, String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = format!("{}/{}", prefix, entry.file_name().to_string_lossy());
        if skip_per_worktree && PER_WORKTREE_REFS.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            walk_loose_refs(&path, &name, skip_per_worktree, out);
        } else if let Ok(content) = fs::read_to_string(&path) {
            out.insert(name, content);
        } else {
            out.insert(name, String::new());
        }
    }
}

/// The ref-name prefixes git iterates for a set of patterns (each pattern up to its first
/// glob character), or `None` (iterate everything) without patterns or with `--ignore-case`.
// Used by `for-each-ref`.
pub fn ref_iteration_prefixes(opts: &ForEachRefOptions) -> Option<Vec<String>> {
    if opts.patterns.is_empty() || opts.ignore_case {
        return None;
    }
    Some(opts.patterns.iter().map(|p| p[..p.find(['*', '?', '[', '\\']).unwrap_or(p.len())].to_string()).collect())
}

fn match_name_as_path(patterns: &[String], refname: &str, ignore_case: bool) -> bool {
    let flags = WM_PATHNAME | if ignore_case { WM_CASEFOLD } else { 0 };
    patterns.iter().any(|p| {
        (p.len() <= refname.len()
            && refname.starts_with(p.as_str())
            && (refname.len() == p.len() || refname.as_bytes()[p.len()] == b'/' || p.ends_with('/')))
            || wildmatch(p, refname, flags)
    })
}

/// Keeps the refs `for-each-ref` would show: matching a pattern (a path prefix ending at a `/`
/// or a glob), not matching an `--exclude`, pointing at a `--points-at` object (directly or
/// through tags), and passing `--contains`/`--no-contains`/`--merged`/`--no-merged` (which
/// also drop refs that don't peel to a commit).
// Used by `for-each-ref`.
pub fn filter_ref_items(items: Vec<RefArrayItem>, opts: &ForEachRefOptions) -> Vec<RefArrayItem> {
    let reach_filtering = !(opts.merged.is_empty() && opts.no_merged.is_empty() && opts.contains.is_empty() && opts.no_contains.is_empty());
    let mut merged_sets: Vec<HashSet<String>> = Vec::new();
    for base in opts.merged.iter().chain(opts.no_merged.iter()) {
        merged_sets.push(collect_ancestors(base).unwrap_or_default());
    }
    let (merged_into, not_merged_into) = merged_sets.split_at(opts.merged.len());

    let mut out = Vec::new();
    for item in items {
        if !opts.patterns.is_empty() && !match_name_as_path(&opts.patterns, &item.refname, opts.ignore_case) {
            continue;
        }
        if !opts.excludes.is_empty() && match_name_as_path(&opts.excludes, &item.refname, opts.ignore_case) {
            continue;
        }
        if !opts.points_at.is_empty() && !points_at(&opts.points_at, &item.oid) {
            continue;
        }
        if reach_filtering {
            let Some(commit) = peel_to_commit(&item.oid) else { continue };
            let ancestors = if opts.contains.is_empty() && opts.no_contains.is_empty() {
                HashSet::new()
            } else {
                collect_ancestors(&commit).unwrap_or_default()
            };
            if !opts.contains.is_empty() && !opts.contains.iter().any(|c| ancestors.contains(c)) {
                continue;
            }
            if opts.no_contains.iter().any(|c| ancestors.contains(c)) {
                continue;
            }
            if !merged_into.is_empty() && !merged_into.iter().any(|set| set.contains(&commit)) {
                continue;
            }
            if !not_merged_into.is_empty() && not_merged_into.iter().any(|set| set.contains(&commit)) {
                continue;
            }
        }
        out.push(item);
    }
    out
}

fn points_at(targets: &[String], oid: &str) -> bool {
    let mut hash = oid.to_string();
    if targets.contains(&hash) {
        return true;
    }
    while let Ok((kind, content)) = read_object(&hash) {
        if kind != "tag" {
            return false;
        }
        match tag_target(&content) {
            Some(target) => {
                if targets.contains(&target) {
                    return true;
                }
                hash = target;
            }
            None => return false,
        }
    }
    false
}

/// Maps each branch checked out in some worktree to that worktree's path (`%(worktreepath)`).
// Used by `for-each-ref`.
pub fn worktree_paths_by_branch() -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Ok(worktrees) = crate::helpers::worktree::list_worktrees() {
        for wt in worktrees {
            if let Some(branch) = wt.branch {
                out.entry(branch).or_insert_with(|| crate::repo::to_git_path_string(&wt.path));
            }
        }
    }
    out
}

fn lstrip_ref(refname: &str, n: i64) -> String {
    let slashes = refname.matches('/').count() as i64;
    let mut remaining = if n < 0 { slashes + n + 1 } else { n };
    let bytes = refname.as_bytes();
    let mut i = 0;
    while remaining > 0 {
        match bytes.get(i) {
            None => return String::new(),
            Some(b'/') => remaining -= 1,
            _ => {}
        }
        i += 1;
    }
    refname[i..].to_string()
}

fn rstrip_ref(refname: &str, n: i64) -> String {
    let slashes = refname.matches('/').count() as i64;
    let mut remaining = if n < 0 { slashes + n + 1 } else { n };
    let mut s = refname.to_string();
    while remaining > 0 {
        match s.rfind('/') {
            Some(p) => s.truncate(p),
            None => return String::new(),
        }
        remaining -= 1;
    }
    s
}

/// Shortens a full ref name to the shortest name that still resolves to it unambiguously,
/// like git's `shorten_unambiguous_ref` (strict unless `core.warnAmbiguousRefs` is false).
// Used by `for-each-ref` (`:short`).
pub fn shorten_unambiguous_ref(refname: &str) -> String {
    let strict = get_config_value("core.warnambiguousrefs").ok().flatten().and_then(|v| parse_maybe_bool(&v)).unwrap_or(true);
    for i in (1..SHORTEN_RULES.len()).rev() {
        let (prefix, suffix) = SHORTEN_RULES[i];
        let Some(short) = refname.strip_prefix(prefix).and_then(|r| r.strip_suffix(suffix)) else { continue };
        if short.is_empty() {
            continue;
        }
        let rules_to_fail = if strict { SHORTEN_RULES.len() } else { i };
        let ambiguous = (0..rules_to_fail).filter(|&j| j != i).any(|j| {
            let (p, s) = SHORTEN_RULES[j];
            ref_exists(&format!("{}{}{}", p, short, s))
        });
        if !ambiguous {
            return short.to_string();
        }
    }
    refname.to_string()
}

fn show_ref_name(show: RefnameShow, refname: &str) -> String {
    match show {
        RefnameShow::Full => refname.to_string(),
        RefnameShow::Short => shorten_unambiguous_ref(refname),
        RefnameShow::Lstrip(n) => lstrip_ref(refname, n),
        RefnameShow::Rstrip(n) => rstrip_ref(refname, n),
    }
}

fn config_values(key: &str) -> Vec<String> {
    let Ok(key) = normalize_key(key) else { return Vec::new() };
    let mut out = Vec::new();
    let mut paths = Vec::new();
    if let Some(global) = global_config_path() {
        paths.push(global);
    }
    paths.push(crate::repo::path(LOCAL_CONFIG_PATH));
    for path in paths {
        for (k, v) in read_entries_from(&path).unwrap_or_default() {
            if k == key {
                out.push(v);
            }
        }
    }
    out
}

fn config_value(key: &str) -> Option<String> {
    get_config_value(key).ok().flatten()
}

/// Maps `name` through refspecs (`[+]src:dst`, `*` globs; `^` negative ones skipped),
/// returning the first destination, like git's `apply_refspecs` / `remote_find_tracking`.
fn apply_refspecs(specs: &[String], name: &str) -> Option<String> {
    for spec in specs {
        let spec = spec.strip_prefix('+').unwrap_or(spec);
        if spec.starts_with('^') {
            continue;
        }
        let Some((src, dst)) = spec.split_once(':') else { continue };
        if dst.is_empty() {
            continue;
        }
        match src.split_once('*') {
            Some((pre, post)) => {
                if name.len() >= pre.len() + post.len() && name.starts_with(pre) && name.ends_with(post) {
                    let matched = &name[pre.len()..name.len() - post.len()];
                    return Some(dst.replacen('*', matched, 1));
                }
            }
            None if src == name => return Some(dst.to_string()),
            None => {}
        }
    }
    None
}

fn branch_remote(branch: &str) -> (String, bool) {
    match config_value(&format!("branch.{}.remote", branch)) {
        Some(r) => (r, true),
        None => ("origin".to_string(), false),
    }
}

fn branch_push_remote(branch: &str) -> (String, bool) {
    if let Some(r) = config_value(&format!("branch.{}.pushremote", branch)) {
        return (r, true);
    }
    if let Some(r) = config_value("remote.pushdefault") {
        return (r, true);
    }
    branch_remote(branch)
}

fn branch_upstream(branch: &str) -> Option<String> {
    let remote = config_value(&format!("branch.{}.remote", branch))?;
    let merge = config_values(&format!("branch.{}.merge", branch)).into_iter().next()?;
    if remote == "." {
        if merge.starts_with("refs/") {
            return Some(merge);
        }
        for (p, s) in SHORTEN_RULES {
            let candidate = format!("{}{}{}", p, merge, s);
            if ref_exists(&candidate) {
                return Some(candidate);
            }
        }
        return Some(merge);
    }
    apply_refspecs(&config_values(&format!("remote.{}.fetch", remote)), &merge)
}

fn branch_push_ref(branch: &str) -> Option<String> {
    let (remote, _) = branch_push_remote(branch);
    let refname = format!("refs/heads/{}", branch);
    let fetch = config_values(&format!("remote.{}.fetch", remote));
    let push = config_values(&format!("remote.{}.push", remote));
    if !push.is_empty() {
        let dst = apply_refspecs(&push, &refname)?;
        return apply_refspecs(&fetch, &dst);
    }
    if config_value(&format!("remote.{}.mirror", remote)).and_then(|v| parse_maybe_bool(&v)).unwrap_or(false) {
        return apply_refspecs(&fetch, &refname);
    }
    match config_value("push.default").map(|v| v.to_ascii_lowercase()).as_deref() {
        Some("nothing") => None,
        Some("matching") | Some("current") => apply_refspecs(&fetch, &refname),
        Some("upstream") | Some("tracking") => branch_upstream(branch),
        _ => {
            let up = branch_upstream(branch)?;
            let cur = apply_refspecs(&fetch, &refname)?;
            (cur == up).then_some(cur)
        }
    }
}

fn tracking_counts(local: &str, base: &str) -> Option<(usize, usize)> {
    let theirs = peel_to_commit(&resolve_ref_chain(base)?.1)?;
    let ours = peel_to_commit(&resolve_ref_chain(local)?.1)?;
    if theirs == ours {
        return Some((0, 0));
    }
    Some(ahead_behind(&ours, &theirs))
}

fn ahead_behind(tip: &str, base: &str) -> (usize, usize) {
    let tip_set = collect_ancestors(tip).unwrap_or_default();
    let base_set = collect_ancestors(base).unwrap_or_default();
    (tip_set.difference(&base_set).count(), base_set.difference(&tip_set).count())
}

fn remote_ref_details(arg: &RefAtomArg, refname: Option<&str>, branch: &str) -> String {
    let RefAtomArg::RemoteRef { show, refname: name_show, nobracket, push, .. } = arg else { return String::new() };
    let base = || if *push { branch_push_ref(branch) } else { branch_upstream(branch) };
    let local = format!("refs/heads/{}", branch);
    match show {
        RemoteRefShow::Ref => refname.map(|r| show_ref_name(*name_show, r)).unwrap_or_default(),
        RemoteRefShow::Track => {
            let text = match base().and_then(|b| tracking_counts(&local, &b)) {
                None => "gone".to_string(),
                Some((0, 0)) => String::new(),
                Some((0, behind)) => format!("behind {}", behind),
                Some((ahead, 0)) => format!("ahead {}", ahead),
                Some((ahead, behind)) => format!("ahead {}, behind {}", ahead, behind),
            };
            if !nobracket && !text.is_empty() { format!("[{}]", text) } else { text }
        }
        RemoteRefShow::TrackShort => match base().and_then(|b| tracking_counts(&local, &b)) {
            None => String::new(),
            Some((0, 0)) => "=".to_string(),
            Some((0, _)) => "<".to_string(),
            Some((_, 0)) => ">".to_string(),
            Some(_) => "<>".to_string(),
        },
        RemoteRefShow::RemoteName => {
            let (remote, explicit) = if *push { branch_push_remote(branch) } else { branch_remote(branch) };
            if explicit { remote } else { String::new() }
        }
        RemoteRefShow::RemoteRef => {
            if !*push {
                config_values(&format!("branch.{}.merge", branch)).into_iter().next().unwrap_or_default()
            } else {
                let (remote, _) = branch_push_remote(branch);
                apply_refspecs(&config_values(&format!("remote.{}.push", remote)), &local).unwrap_or_default()
            }
        }
    }
}

/// Computes every atom's value for one ref, like git's `populate_value`: ref-level atoms first
/// (`%(refname)`, `%(upstream)`, `%(HEAD)`, colors, `%(if)`...), then values read from the
/// object the ref names, and with `%(*...)` atoms from the object an annotated tag points at.
/// Atoms that don't apply are empty. Errors are git's fatal messages.
// Used by `for-each-ref`.
pub fn populate_ref_values(item: &RefArrayItem, atoms: &[RefFilterAtom], worktrees: &HashMap<String, String>) -> Result<Vec<RefAtomValue>, String> {
    let mut values: Vec<Option<RefAtomValue>> = vec![None; atoms.len()];
    let text = |s: String| Some(RefAtomValue { s: s.into_bytes(), ..Default::default() });
    let mut reported_non_commit = false;
    let symref_target = if item.symref { resolve_ref_chain(&item.refname).map(|(n, _, _)| n).unwrap_or_default() } else { String::new() };

    for (i, atom) in atoms.iter().enumerate() {
        use RefAtomKind as K;
        let refname = match atom.kind {
            K::Refname => {
                let RefAtomArg::Refname(show) = &atom.arg else { continue };
                show_ref_name(*show, &item.refname)
            }
            K::Symref => {
                let RefAtomArg::Refname(show) = &atom.arg else { continue };
                if item.symref { show_ref_name(*show, &symref_target) } else { String::new() }
            }
            K::WorktreePath => {
                values[i] = text(if item.refname.starts_with("refs/heads/") { worktrees.get(&item.refname).cloned().unwrap_or_default() } else { String::new() });
                continue;
            }
            K::Upstream => {
                let s = match item.refname.strip_prefix("refs/heads/") {
                    Some(branch) => match branch_upstream(branch) {
                        Some(up) => remote_ref_details(&atom.arg, Some(&up), branch),
                        None => String::new(),
                    },
                    None => String::new(),
                };
                values[i] = text(s);
                continue;
            }
            K::Push => {
                let s = match item.refname.strip_prefix("refs/heads/") {
                    Some(branch) => {
                        let push_remote = matches!(atom.arg, RefAtomArg::RemoteRef { push_remote: true, .. });
                        if push_remote {
                            remote_ref_details(&atom.arg, None, branch)
                        } else {
                            match branch_push_ref(branch) {
                                Some(dst) => remote_ref_details(&atom.arg, Some(&dst), branch),
                                None => String::new(),
                            }
                        }
                    }
                    None => String::new(),
                };
                values[i] = text(s);
                continue;
            }
            K::Color => {
                let RefAtomArg::Color(c) = &atom.arg else { continue };
                values[i] = text(c.clone());
                continue;
            }
            K::Flag => {
                let mut flags = Vec::new();
                if item.symref {
                    flags.push("symref");
                }
                if item.packed {
                    flags.push("packed");
                }
                values[i] = text(flags.join(","));
                continue;
            }
            K::ObjectName if !atom.deref => {
                let RefAtomArg::Oid(len) = &atom.arg else { continue };
                values[i] = text(show_oid(&item.oid, *len));
                continue;
            }
            K::Head => {
                let RefAtomArg::Head(head) = &atom.arg else { continue };
                values[i] = text(if head.as_deref() == Some(item.refname.as_str()) { "*" } else { " " }.to_string());
                continue;
            }
            K::Align | K::End | K::Then | K::Else | K::Rest => {
                values[i] = text(String::new());
                continue;
            }
            K::If => {
                let rest = atom.name.strip_prefix("if:").unwrap_or("");
                values[i] = text(rest.to_string());
                continue;
            }
            K::AheadBehind => {
                let RefAtomArg::AheadBehind(base) = &atom.arg else { continue };
                let tip = match peel_to_commit_reporting(&item.oid) {
                    Ok(tip) => Some(tip),
                    Err(message) => {
                        if !reported_non_commit {
                            eprintln!("{}", message);
                            reported_non_commit = true;
                        }
                        None
                    }
                };
                let s = match tip {
                    Some(tip) => {
                        let (ahead, behind) = ahead_behind(&tip, base);
                        format!("{} {}", ahead, behind)
                    }
                    None => String::new(),
                };
                values[i] = text(s);
                continue;
            }
            _ => continue,
        };
        values[i] = text(if atom.deref { format!("{}^{{}}", refname) } else { refname });
    }

    let need_tagged = atoms.iter().any(|a| a.deref);
    let wants = |deref: bool| {
        atoms.iter().any(|a| {
            a.deref == deref
                && (a.needs_content || matches!(a.kind, RefAtomKind::ObjectType | RefAtomKind::ObjectSize | RefAtomKind::DeltaBase) || (deref && a.kind == RefAtomKind::ObjectName))
        })
    };
    if need_tagged || wants(false) || wants(true) {
        let (kind, content) = read_object(&item.oid).map_err(|_| format!("missing object {} for {}", item.oid, item.refname))?;
        grab_values(&mut values, atoms, false, &item.oid, &kind, &content)?;
        if need_tagged && kind == "tag" {
            if let Some(target) = tag_target(&content) {
                let (dkind, dcontent) = read_object(&target).map_err(|_| format!("missing object {} for {}", target, item.refname))?;
                grab_values(&mut values, atoms, true, &target, &dkind, &dcontent)?;
            }
        }
    }
    Ok(values.into_iter().map(Option::unwrap_or_default).collect())
}

fn show_oid(oid: &str, len: Option<usize>) -> String {
    match len {
        None => oid.to_string(),
        Some(n) => abbreviate_hash_min(oid, n),
    }
}

fn grab_values(values: &mut [Option<RefAtomValue>], atoms: &[RefFilterAtom], deref: bool, oid: &str, kind: &str, content: &[u8]) -> Result<(), String> {
    let parents: Vec<String> = content
        .split(|&b| b == b'\n')
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.strip_prefix(b"parent "))
        .map(|p| String::from_utf8_lossy(p).to_string())
        .collect();
    let set = |values: &mut [Option<RefAtomValue>], i: usize, s: Vec<u8>, value: u64| {
        values[i] = Some(RefAtomValue { s, value, binary: false });
    };
    for (i, atom) in atoms.iter().enumerate() {
        if atom.deref != deref {
            continue;
        }
        let bare = if deref { &atom.name[1..] } else { atom.name.as_str() };
        use RefAtomKind as K;
        match atom.kind {
            K::ObjectType => set(values, i, kind.as_bytes().to_vec(), 0),
            K::ObjectSize => {
                let size = if atom.arg == (RefAtomArg::ObjectSize { disk: true }) {
                    fs::metadata(crate::repo::path(format!(".git/objects/{}/{}", &oid[..2], &oid[2..]))).map(|m| m.len()).unwrap_or(0)
                } else {
                    content.len() as u64
                };
                set(values, i, size.to_string().into_bytes(), size);
            }
            K::DeltaBase => set(values, i, "0".repeat(40).into_bytes(), 0),
            K::ObjectName if deref => {
                let RefAtomArg::Oid(len) = &atom.arg else { continue };
                set(values, i, show_oid(oid, *len).into_bytes(), 0);
            }
            K::Raw => {
                if atom.arg == (RefAtomArg::Raw { size: true }) {
                    set(values, i, content.len().to_string().into_bytes(), content.len() as u64);
                } else {
                    values[i] = Some(RefAtomValue { s: content.to_vec(), value: 0, binary: true });
                }
            }
            _ => {}
        }
        if kind == "tag" {
            match atom.kind {
                K::Tag => set(values, i, header_value(content, "tag").unwrap_or_default().into_bytes(), 0),
                K::Type => {
                    if let Some(t) = header_value(content, "type") {
                        set(values, i, t.into_bytes(), 0);
                    }
                }
                K::Object => {
                    if let Some(t) = tag_target(content) {
                        set(values, i, t.into_bytes(), 0);
                    }
                }
                _ => {}
            }
        }
        if kind == "commit" {
            match atom.kind {
                K::Tree => {
                    let RefAtomArg::Oid(len) = &atom.arg else { continue };
                    set(values, i, show_oid(&header_value(content, "tree").unwrap_or_default(), *len).into_bytes(), 0);
                }
                K::NumParent => set(values, i, parents.len().to_string().into_bytes(), parents.len() as u64),
                K::Parent => {
                    let RefAtomArg::Oid(len) = &atom.arg else { continue };
                    let shown: Vec<String> = parents.iter().map(|p| show_oid(p, *len)).collect();
                    set(values, i, shown.join(" ").into_bytes(), 0);
                }
                K::Signature => {
                    let RefAtomArg::Signature(opt) = &atom.arg else { continue };
                    let s = match opt.as_str() {
                        "grade" => "N",
                        "trustlevel" => "undefined",
                        _ => "",
                    };
                    set(values, i, s.as_bytes().to_vec(), 0);
                }
                _ => {}
            }
        }
        if kind == "tag" || kind == "commit" {
            if bare == "body" || bare.starts_with("subject") || bare.starts_with("trailers") || bare.starts_with("contents") {
                if let RefAtomArg::Contents(part) = &atom.arg {
                    let (s, value) = grab_message_value(content, part);
                    set(values, i, s, value);
                }
            }
            if atom.kind == K::Describe {
                let RefAtomArg::Describe { tags, abbrev, matches, excludes } = &atom.arg else { continue };
                set(values, i, describe_value(oid, tags.unwrap_or(false), *abbrev, matches, excludes).into_bytes(), 0);
            }
        }
    }
    let people: &[&str] = match kind {
        "tag" => &["tagger"],
        "commit" => &["author", "committer"],
        _ => &[],
    };
    for who in people {
        grab_person(values, atoms, deref, who, content)?;
    }
    Ok(())
}

fn describe_value(oid: &str, all_tags: bool, abbrev: Option<usize>, matches: &[String], excludes: &[String]) -> String {
    let Some(commit) = peel_to_commit(oid) else { return String::new() };
    let accept = |name: &str| -> bool {
        if excludes.iter().any(|p| wildmatch(p, name, 0)) {
            return false;
        }
        matches.is_empty() || matches.iter().any(|p| wildmatch(p, name, 0))
    };
    match describe_commit_matching(&commit, all_tags, &accept) {
        Ok(Some((name, depth))) => {
            let abbrev = abbrev.unwrap_or(7);
            if depth == 0 || abbrev == 0 {
                name
            } else {
                format!("{}-{}-g{}", name, depth, abbreviate_hash_min(&commit, abbrev))
            }
        }
        _ => String::new(),
    }
}

fn find_wholine<'a>(who: &str, buf: &'a [u8]) -> &'a [u8] {
    let mut pos = 0;
    while pos < buf.len() {
        let line = &buf[pos..];
        if line.starts_with(who.as_bytes()) && line.get(who.len()) == Some(&b' ') {
            return &line[who.len() + 1..];
        }
        match line.iter().position(|&b| b == b'\n') {
            None => return b"",
            Some(eol) => {
                pos += eol + 1;
                if buf.get(pos) == Some(&b'\n') {
                    return b"";
                }
            }
        }
    }
    b""
}

fn line_of(buf: &[u8]) -> &[u8] {
    &buf[..buf.iter().position(|&b| b == b'\n').unwrap_or(buf.len())]
}

fn grab_person(values: &mut [Option<RefAtomValue>], atoms: &[RefFilterAtom], deref: bool, who: &str, buf: &[u8]) -> Result<(), String> {
    let wholine = find_wholine(who, buf);
    for (i, atom) in atoms.iter().enumerate() {
        if atom.deref != deref {
            continue;
        }
        let name = if deref { &atom.name[1..] } else { atom.name.as_str() };
        let Some(rest) = name.strip_prefix(who) else { continue };
        let s = if rest.is_empty() {
            line_of(wholine).to_vec()
        } else if rest.starts_with("name") {
            let line = line_of(wholine);
            match line.windows(2).position(|w| w == b" <") {
                Some(p) => line[..p].to_vec(),
                None => Vec::new(),
            }
        } else if rest.starts_with("email") {
            let RefAtomArg::Email { trim, localpart } = atom.arg else { continue };
            copy_email(wholine, trim, localpart)
        } else if rest.starts_with("date") {
            let (s, value) = grab_date(wholine, name)?;
            values[i] = Some(RefAtomValue { s, value, binary: false });
            continue;
        } else {
            continue;
        };
        values[i] = Some(RefAtomValue { s, value: 0, binary: false });
    }
    if who != "tagger" && who != "committer" {
        return Ok(());
    }
    for (i, atom) in atoms.iter().enumerate() {
        if atom.deref != deref {
            continue;
        }
        let name = if deref { &atom.name[1..] } else { atom.name.as_str() };
        match atom.kind {
            RefAtomKind::CreatorDate => {
                let (s, value) = grab_date(wholine, name)?;
                values[i] = Some(RefAtomValue { s, value, binary: false });
            }
            RefAtomKind::Creator => values[i] = Some(RefAtomValue { s: line_of(wholine).to_vec(), value: 0, binary: false }),
            _ => {}
        }
    }
    Ok(())
}

fn copy_email(buf: &[u8], trim: bool, localpart: bool) -> Vec<u8> {
    let Some(start) = buf.iter().position(|&b| b == b'<') else { return Vec::new() };
    let email = if trim || localpart { &buf[start + 1..] } else { &buf[start..] };
    let gt = email.iter().position(|&b| b == b'>');
    let end = if localpart {
        email.iter().position(|&b| b == b'@').or(gt)
    } else if trim {
        gt
    } else {
        gt.map(|p| p + 1)
    };
    match end {
        Some(e) => email[..e].to_vec(),
        None => Vec::new(),
    }
}

fn grab_date(buf: &[u8], atom_name: &str) -> Result<(Vec<u8>, u64), String> {
    let mode = match atom_name.find(':') {
        Some(p) => parse_ref_date_mode(&atom_name[p + 1..])?,
        None => RefDateMode::default(),
    };
    let Some(p) = buf.windows(2).position(|w| w == b"> ") else { return Ok((Vec::new(), 0)) };
    let rest = &buf[p + 2..];
    let digits_end = rest.iter().position(|b| !b.is_ascii_digit()).unwrap_or(rest.len());
    let Ok(timestamp) = String::from_utf8_lossy(&rest[..digits_end]).parse::<i64>() else { return Ok((Vec::new(), 0)) };
    let line_end = rest[digits_end..].iter().position(|&b| b == b'\n').map(|e| e + digits_end).unwrap_or(rest.len());
    let zone = String::from_utf8_lossy(&rest[digits_end..line_end]).to_string();
    let tz = parse_strtol_prefix(&zone);
    Ok((show_ref_date(timestamp, tz, &mode).into_bytes(), timestamp as u64))
}

fn parse_strtol_prefix(text: &str) -> i64 {
    let t = text.trim_start();
    let (negative, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let n: i64 = rest[..end].parse().unwrap_or(0);
    if negative { -n } else { n }
}

/// Parses a date format like git's `parse_date_format` (`relative`, `iso`, `iso-strict`, `rfc`,
/// `short`, `default`, `human`, `raw`, `unix`, `format:<strftime>`, `local`, `auto:<mode>`,
/// each optionally `-local`).
// Used by `for-each-ref` (`%(authordate:<format>)` and friends).
pub fn parse_ref_date_mode(format: &str) -> Result<RefDateMode, String> {
    let mut format = format.to_string();
    if let Some(rest) = format.strip_prefix("auto:") {
        format = if std::io::stdout().is_terminal() { rest.to_string() } else { "default".to_string() };
    }
    if format == "local" {
        format = "default-local".to_string();
    }
    let kinds: &[(&str, RefDateKind)] = &[
        ("relative", RefDateKind::Relative),
        ("iso8601-strict", RefDateKind::IsoStrict),
        ("iso-strict", RefDateKind::IsoStrict),
        ("iso8601", RefDateKind::Iso),
        ("iso", RefDateKind::Iso),
        ("rfc2822", RefDateKind::Rfc),
        ("rfc", RefDateKind::Rfc),
        ("short", RefDateKind::Short),
        ("default", RefDateKind::Normal),
        ("human", RefDateKind::Human),
        ("raw", RefDateKind::Raw),
        ("unix", RefDateKind::Unix),
        ("format", RefDateKind::Strftime),
    ];
    let Some((prefix, kind)) = kinds.iter().find(|(p, _)| format.starts_with(p)) else {
        return Err(format!("unknown date format {}", format));
    };
    let mut rest = &format[prefix.len()..];
    let mut mode = RefDateMode { kind: *kind, ..Default::default() };
    if let Some(r) = rest.strip_prefix("-local") {
        mode.local = true;
        rest = r;
    }
    if mode.kind == RefDateKind::Strftime {
        match rest.strip_prefix(':') {
            Some(fmt) => mode.strftime = fmt.to_string(),
            None => return Err(format!("date format missing colon separator: {}", format)),
        }
    } else if !rest.is_empty() {
        return Err(format!("unknown date format {}", format));
    }
    Ok(mode)
}

#[derive(Clone, Copy, Default)]
struct Tm {
    year: i64,
    mon: i64,
    mday: i64,
    hour: i64,
    min: i64,
    sec: i64,
    wday: i64,
    yday: i64,
}

fn tz_minutes(tz: i64) -> i64 {
    let m = tz.abs();
    let m = (m / 100) * 60 + m % 100;
    if tz < 0 { -m } else { m }
}

fn gmtime(t: i64) -> Tm {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (jan1, _, _) = (days_from_civil(year, 1, 1), 0, 0);
    Tm {
        year,
        mon: month - 1,
        mday: day,
        hour: secs / 3600,
        min: secs % 3600 / 60,
        sec: secs % 60,
        wday: (days + 4).rem_euclid(7),
        yday: days - jan1,
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn now_seconds() -> i64 {
    if let Ok(v) = std::env::var("GIT_TEST_DATE_NOW") {
        return parse_strtol_prefix(&v);
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn relative_date(time: i64) -> String {
    let now = now_seconds();
    if now < time {
        return "in the future".to_string();
    }
    let plural = |n: i64, unit: &str| format!("{} {}{} ago", n, unit, if n == 1 { "" } else { "s" });
    let mut diff = now - time;
    if diff < 90 {
        return plural(diff, "second");
    }
    diff = (diff + 30) / 60;
    if diff < 90 {
        return plural(diff, "minute");
    }
    diff = (diff + 30) / 60;
    if diff < 36 {
        return plural(diff, "hour");
    }
    diff = (diff + 12) / 24;
    if diff < 14 {
        return plural(diff, "day");
    }
    if diff < 70 {
        return plural((diff + 3) / 7, "week");
    }
    if diff < 365 {
        return plural((diff + 15) / 30, "month");
    }
    if diff < 1825 {
        let total = (diff * 12 * 2 + 365) / (365 * 2);
        let (years, months) = (total / 12, total % 12);
        if months > 0 {
            return format!("{} year{}, {} month{} ago", years, if years == 1 { "" } else { "s" }, months, if months == 1 { "" } else { "s" });
        }
        return plural(years, "year");
    }
    plural((diff + 183) / 365, "year")
}

/// Formats a timestamp and `+hhmm`-style zone (as git's decimal parse, `-0100` = -100) like
/// git's `show_date`. The local time zone is taken to be UTC.
// Used by `for-each-ref` (date atoms).
pub fn show_ref_date(time: i64, tz: i64, mode: &RefDateMode) -> String {
    if mode.kind == RefDateKind::Unix {
        return time.to_string();
    }
    let tz = if mode.local { 0 } else { tz };
    if mode.kind == RefDateKind::Raw {
        return format!("{} {}", time, format_tz(tz));
    }
    if mode.kind == RefDateKind::Relative {
        return relative_date(time);
    }
    let tm = gmtime(time + tz_minutes(tz) * 60);
    match mode.kind {
        RefDateKind::Short => format!("{:04}-{:02}-{:02}", tm.year, tm.mon + 1, tm.mday),
        RefDateKind::Iso => format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}", tm.year, tm.mon + 1, tm.mday, tm.hour, tm.min, tm.sec, format_tz(tz)),
        RefDateKind::IsoStrict => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
            tm.year,
            tm.mon + 1,
            tm.mday,
            tm.hour,
            tm.min,
            tm.sec,
            if tz >= 0 { '+' } else { '-' },
            tz.abs() / 100,
            tz.abs() % 100
        ),
        RefDateKind::Rfc => format!(
            "{}, {} {} {} {:02}:{:02}:{:02} {}",
            &WEEKDAYS[tm.wday as usize][..3],
            tm.mday,
            &MONTHS[tm.mon as usize][..3],
            tm.year,
            tm.hour,
            tm.min,
            tm.sec,
            format_tz(tz)
        ),
        RefDateKind::Strftime => strftime(&mode.strftime, &tm, tz, time, mode.local),
        RefDateKind::Human => {
            let now = gmtime(now_seconds());
            show_date_normal(time, &tm, tz, Some((&now, 0)), mode.local)
        }
        _ => show_date_normal(time, &tm, tz, None, mode.local),
    }
}

fn format_tz(tz: i64) -> String {
    format!("{}{:04}", if tz < 0 { '-' } else { '+' }, tz.abs())
}

fn show_date_normal(time: i64, tm: &Tm, tz: i64, human: Option<(&Tm, i64)>, local: bool) -> String {
    let (human_tm, human_tz) = match human {
        Some((t, z)) => (*t, z),
        None => (Tm { year: 1900, ..Default::default() }, -1),
    };
    let mut hide_tz = local || tz == human_tz;
    let hide_year = tm.year == human_tm.year;
    let (mut hide_date, mut hide_wday, mut hide_time, mut hide_seconds) = (false, false, false, false);
    if hide_year && tm.mon == human_tm.mon {
        if tm.mday > human_tm.mday {
        } else if tm.mday == human_tm.mday {
            hide_date = true;
            hide_wday = true;
        } else if tm.mday + 5 > human_tm.mday {
            hide_date = true;
        }
    }
    if hide_wday {
        return relative_date(time);
    }
    if human_tm.year != 1900 {
        hide_seconds = true;
        hide_tz |= !hide_date;
        hide_wday = !hide_year;
        hide_time = !hide_year;
    }
    let mut out = String::new();
    if !hide_wday {
        out.push_str(&format!("{} ", &WEEKDAYS[tm.wday as usize][..3]));
    }
    if !hide_date {
        out.push_str(&format!("{} {} ", &MONTHS[tm.mon as usize][..3], tm.mday));
    }
    if !hide_time {
        out.push_str(&format!("{:02}:{:02}", tm.hour, tm.min));
        if !hide_seconds {
            out.push_str(&format!(":{:02}", tm.sec));
        }
    } else {
        out.truncate(out.trim_end().len());
    }
    if !hide_year {
        out.push_str(&format!(" {}", tm.year));
    }
    if !hide_tz {
        out.push_str(&format!(" {}", format_tz(tz)));
    }
    out
}

fn iso_week(tm: &Tm) -> (i64, i64) {
    let wday = (tm.wday + 6) % 7;
    let mut year = tm.year;
    let mut week = (tm.yday - wday + 10) / 7;
    let weeks_in = |y: i64| {
        let jan1 = (days_from_civil(y, 1, 1) + 4).rem_euclid(7);
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        if jan1 == 4 || (leap && jan1 == 3) { 53 } else { 52 }
    };
    if week < 1 {
        year -= 1;
        week = weeks_in(year);
    } else if week > weeks_in(year) {
        year += 1;
        week = 1;
    }
    (year, week)
}

fn strftime(fmt: &str, tm: &Tm, tz: i64, time: i64, local: bool) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    let hour12 = if tm.hour % 12 == 0 { 12 } else { tm.hour % 12 };
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(spec) = chars.next() else {
            out.push('%');
            break;
        };
        let piece = match spec {
            '%' => "%".to_string(),
            'a' => WEEKDAYS[tm.wday as usize][..3].to_string(),
            'A' => WEEKDAYS[tm.wday as usize].to_string(),
            'b' | 'h' => MONTHS[tm.mon as usize][..3].to_string(),
            'B' => MONTHS[tm.mon as usize].to_string(),
            'c' => format!(
                "{} {} {:>2} {:02}:{:02}:{:02} {}",
                &WEEKDAYS[tm.wday as usize][..3],
                &MONTHS[tm.mon as usize][..3],
                tm.mday,
                tm.hour,
                tm.min,
                tm.sec,
                tm.year
            ),
            'C' => format!("{:02}", tm.year.div_euclid(100)),
            'd' => format!("{:02}", tm.mday),
            'D' => format!("{:02}/{:02}/{:02}", tm.mon + 1, tm.mday, tm.year.rem_euclid(100)),
            'e' => format!("{:>2}", tm.mday),
            'F' => format!("{:04}-{:02}-{:02}", tm.year, tm.mon + 1, tm.mday),
            'g' => format!("{:02}", iso_week(tm).0.rem_euclid(100)),
            'G' => iso_week(tm).0.to_string(),
            'H' => format!("{:02}", tm.hour),
            'I' => format!("{:02}", hour12),
            'j' => format!("{:03}", tm.yday + 1),
            'k' => format!("{:>2}", tm.hour),
            'l' => format!("{:>2}", hour12),
            'm' => format!("{:02}", tm.mon + 1),
            'M' => format!("{:02}", tm.min),
            'n' => "\n".to_string(),
            'p' => if tm.hour < 12 { "AM" } else { "PM" }.to_string(),
            'P' => if tm.hour < 12 { "am" } else { "pm" }.to_string(),
            'r' => format!("{:02}:{:02}:{:02} {}", hour12, tm.min, tm.sec, if tm.hour < 12 { "AM" } else { "PM" }),
            'R' => format!("{:02}:{:02}", tm.hour, tm.min),
            's' => time.to_string(),
            'S' => format!("{:02}", tm.sec),
            't' => "\t".to_string(),
            'T' => format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec),
            'u' => (if tm.wday == 0 { 7 } else { tm.wday }).to_string(),
            'U' => format!("{:02}", (tm.yday + 7 - tm.wday) / 7),
            'V' => format!("{:02}", iso_week(tm).1),
            'w' => tm.wday.to_string(),
            'W' => format!("{:02}", (tm.yday + 7 - (tm.wday + 6) % 7) / 7),
            'x' => format!("{:02}/{:02}/{:02}", tm.mon + 1, tm.mday, tm.year.rem_euclid(100)),
            'X' => format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec),
            'y' => format!("{:02}", tm.year.rem_euclid(100)),
            'Y' => tm.year.to_string(),
            'z' => format_tz(tz),
            'Z' => if local { "UTC".to_string() } else { String::new() },
            other => format!("%{}", other),
        };
        out.push_str(&piece);
    }
    out
}

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

fn grab_message_value(buf: &[u8], part: &RefContentsPart) -> (Vec<u8>, u64) {
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

fn c_string(s: &[u8]) -> &[u8] {
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

/// The configured `versionsort.suffix` values (or the older `versionsort.prereleaseSuffix`,
/// with git's warning when both are set).
// Used by `for-each-ref` (`--sort=version:<key>`).
pub fn version_suffixes() -> Vec<String> {
    let new = config_values("versionsort.suffix");
    let old = config_values("versionsort.prereleasesuffix");
    if !new.is_empty() && !old.is_empty() {
        eprintln!("warning: ignoring versionsort.prereleasesuffix because versionsort.suffix is set");
    }
    if !new.is_empty() { new } else { old }
}

/// Compares two strings as version numbers like git's `versioncmp` (digit runs compare
/// numerically; `versionsort.suffix` values such as `-rc` sort before the release).
// Used by `for-each-ref` (`--sort=version:<key>`).
pub fn versioncmp(s1: &[u8], s2: &[u8], suffixes: &[String]) -> i32 {
    const S_N: usize = 0;
    const S_I: usize = 3;
    const S_F: usize = 6;
    const S_Z: usize = 9;
    const CMP: i32 = 2;
    const LEN: i32 = 3;
    const NEXT_STATE: [usize; 12] = [S_N, S_I, S_Z, S_N, S_I, S_I, S_N, S_F, S_F, S_N, S_F, S_Z];
    const RESULT: [i32; 36] = [
        CMP, CMP, CMP, CMP, LEN, CMP, CMP, CMP, CMP, //
        CMP, -1, -1, 1, LEN, LEN, 1, LEN, LEN, //
        CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, CMP, //
        CMP, 1, 1, -1, CMP, CMP, -1, CMP, CMP,
    ];
    let s1 = c_string(s1);
    let s2 = c_string(s2);
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);
    let class = |c: u8| usize::from(c == b'0') + usize::from(c.is_ascii_digit());
    let (mut p1, mut p2) = (0, 0);
    let mut c1 = at(s1, p1);
    let mut c2 = at(s2, p2);
    p1 += 1;
    p2 += 1;
    let mut state = S_N + class(c1);
    let mut diff;
    loop {
        diff = c1 as i32 - c2 as i32;
        if diff != 0 {
            break;
        }
        if c1 == 0 {
            return 0;
        }
        state = NEXT_STATE[state];
        c1 = at(s1, p1);
        c2 = at(s2, p2);
        p1 += 1;
        p2 += 1;
        state += class(c1);
    }
    if !suffixes.is_empty() {
        if let Some(d) = swap_prereleases(s1, s2, p1 - 1, suffixes) {
            return d;
        }
    }
    match RESULT[state * 3 + class(c2)] {
        CMP => diff,
        LEN => {
            loop {
                let a = at(s1, p1);
                p1 += 1;
                if !a.is_ascii_digit() {
                    break;
                }
                let b = at(s2, p2);
                p2 += 1;
                if !b.is_ascii_digit() {
                    return 1;
                }
            }
            if at(s2, p2).is_ascii_digit() { -1 } else { diff }
        }
        other => other,
    }
}

fn swap_prereleases(s1: &[u8], s2: &[u8], off: usize, suffixes: &[String]) -> Option<i32> {
    let find = |s: &[u8], suffix: &[u8], start: usize, conf_pos: i64, m: &mut (i64, usize, i64)| {
        let end = if m.2 < suffix.len() as i64 { m.1 as i64 } else { m.1 as i64 - 1 };
        let mut i = start as i64;
        while i <= end {
            if s.get(i as usize..).is_some_and(|r| r.starts_with(suffix)) {
                *m = (conf_pos, i as usize, suffix.len() as i64);
                break;
            }
            i += 1;
        }
    };
    let mut m1 = (-1i64, off, -1i64);
    let mut m2 = (-1i64, off, -1i64);
    for (i, suffix) in suffixes.iter().enumerate() {
        let suffix = suffix.as_bytes();
        let start = off.saturating_sub(suffix.len());
        find(s1, suffix, start, i as i64, &mut m1);
        find(s2, suffix, start, i as i64, &mut m2);
    }
    if m1.0 == -1 && m2.0 == -1 {
        return None;
    }
    if m1.0 == m2.0 {
        return None;
    }
    Some(if m1.0 >= 0 && m2.0 >= 0 {
        (m1.0 - m2.0) as i32
    } else if m1.0 >= 0 {
        -1
    } else {
        1
    })
}

fn strcasecmp(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
    let la = a.iter().map(|c| c.to_ascii_lowercase());
    let lb = b.iter().map(|c| c.to_ascii_lowercase());
    la.cmp(lb)
}

/// Orders two refs by the sort keys (primary first), like git's `compare_refs`: version,
/// string (case-insensitive with `--ignore-case`) or numeric comparison, `-` reversing, with
/// the ref name as the final tie-breaker.
// Used by `for-each-ref`.
pub fn compare_ref_items(
    a: (&RefArrayItem, &[RefAtomValue]),
    b: (&RefArrayItem, &[RefAtomValue]),
    sorts: &[(usize, bool, bool)],
    atoms: &[RefFilterAtom],
    ignore_case: bool,
    suffixes: &[String],
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    for &(at, reverse, version) in sorts {
        let (va, vb) = (&a.1[at], &b.1[at]);
        let cmp = if version {
            versioncmp(&va.s, &vb.s, suffixes).cmp(&0)
        } else if !atoms[at].numeric {
            if !va.binary && !vb.binary {
                if ignore_case { strcasecmp(c_string(&va.s), c_string(&vb.s)) } else { c_string(&va.s).cmp(c_string(&vb.s)) }
            } else if ignore_case {
                strcasecmp(&va.s, &vb.s)
            } else {
                va.s.cmp(&vb.s)
            }
        } else {
            va.value.cmp(&vb.value)
        };
        let cmp = if reverse { cmp.reverse() } else { cmp };
        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    if ignore_case { strcasecmp(a.0.refname.as_bytes(), b.0.refname.as_bytes()) } else { a.0.refname.cmp(&b.0.refname) }
}
