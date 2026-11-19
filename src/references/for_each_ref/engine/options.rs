//! Argument parsing for `for-each-ref`, like git's parse-options: unique prefixes, `--no-` negations, bundled switches.

use crate::core::rev_parse::resolve_rev;
use crate::references::{
    for_each_ref::engine::{exit_error, fatal, FOR_EACH_REF_DEFAULT_FORMAT, FOR_EACH_REF_USAGE, LONG_OPTIONS, usage_error},
    for_each_ref::engine::atoms::peel_to_commit_reporting,
    for_each_ref::types::{ForEachRefExit, ForEachRefOptions, RefQuoteStyle},
};

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
