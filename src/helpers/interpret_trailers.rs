use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use crate::helpers::config::read_all_config_entries;
use crate::objects::{
    InterpretTrailersOptions, NewTrailer, TrailerArg, TrailerConf, TrailerConfig, TrailerIfExists, TrailerIfMissing, TrailerItem, TrailerWhere,
};

/// git's usage text for `interpret-trailers`, printed for `-h` and after option errors (exit 129).
pub const INTERPRET_TRAILERS_USAGE: &str = "usage: rgit interpret-trailers [--in-place] [--trim-empty]
                               [(--trailer (<key>|<keyAlias>)[(=|:)<value>])...]
                               [--parse] [<file>...]

    --[no-]in-place       edit files in place
    --[no-]trim-empty     trim empty trailers
    --[no-]where <placement>
                          where to place the new trailer
    --[no-]if-exists <action>
                          action if trailer already exists
    --[no-]if-missing <action>
                          action if trailer is missing
    --[no-]only-trailers  output only the trailers
    --[no-]only-input     do not apply trailer.* configuration variables
    --[no-]unfold         reformat multiline trailer values as single-line values
    --parse               alias for --only-trailers --only-input --unfold
    --[no-]divider        do not treat \"---\" as the end of input
    --[no-]trailer <trailer>
                          trailer(s) to add
";

/// The character that starts a comment line in a commit message (git's default `core.commentChar`).
const COMMENT_LINE_CHAR: u8 = b'#';

/// Line prefixes git itself writes; one of them makes a mixed block count as trailers.
const GIT_GENERATED_PREFIXES: [&[u8]; 2] = [b"Signed-off-by: ", b"(cherry picked from commit "];

/// The scissors line `commit -v` / `--cleanup=scissors` puts before the diff, with its leading newline.
const CUT_LINE: &[u8] = b"\n# ------------------------ >8 ------------------------\n";

/// git's `isspace`: space, tab, LF or CR.
// Used by `interpret-trailers` (internal to this file's helpers).
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// A copy of `bytes` without leading and trailing whitespace (git's `strbuf_trim`).
// Used by `interpret-trailers` (internal to this file's helpers).
fn trimmed(bytes: &[u8]) -> Vec<u8> {
    let start = bytes.iter().position(|&b| !is_space(b)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|&b| !is_space(b)).map(|p| p + 1).unwrap_or(start);
    bytes[start..end].to_vec()
}

/// C's `strncasecmp(a, b, n) == 0` on byte strings (a shorter string only matches itself).
// Used by `interpret-trailers` (internal to this file's helpers).
fn equal_prefix_ignore_case(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if !x.eq_ignore_ascii_case(&y) {
            return false;
        }
        if x == 0 {
            break;
        }
    }
    true
}

/// git's `trailer_set_where`: `after`, `before`, `end` or `start` (any case).
// Used by `interpret-trailers` (`--where`, `trailer.where`, `trailer.<token>.where`).
pub fn parse_trailer_where(value: &str) -> Option<TrailerWhere> {
    match value.to_ascii_lowercase().as_str() {
        "after" => Some(TrailerWhere::After),
        "before" => Some(TrailerWhere::Before),
        "end" => Some(TrailerWhere::End),
        "start" => Some(TrailerWhere::Start),
        _ => None,
    }
}

/// git's `trailer_set_if_exists`: `addIfDifferent`, `addIfDifferentNeighbor`, `add`,
/// `replace` or `doNothing` (any case).
// Used by `interpret-trailers` (`--if-exists`, `trailer.ifexists`, `trailer.<token>.ifexists`).
pub fn parse_trailer_if_exists(value: &str) -> Option<TrailerIfExists> {
    match value.to_ascii_lowercase().as_str() {
        "addifdifferent" => Some(TrailerIfExists::AddIfDifferent),
        "addifdifferentneighbor" => Some(TrailerIfExists::AddIfDifferentNeighbor),
        "add" => Some(TrailerIfExists::Add),
        "replace" => Some(TrailerIfExists::Replace),
        "donothing" => Some(TrailerIfExists::DoNothing),
        _ => None,
    }
}

/// git's `trailer_set_if_missing`: `doNothing` or `add` (any case).
// Used by `interpret-trailers` (`--if-missing`, `trailer.ifmissing`, `trailer.<token>.ifmissing`).
pub fn parse_trailer_if_missing(value: &str) -> Option<TrailerIfMissing> {
    match value.to_ascii_lowercase().as_str() {
        "donothing" => Some(TrailerIfMissing::DoNothing),
        "add" => Some(TrailerIfMissing::Add),
        _ => None,
    }
}

/// Parses `interpret-trailers` arguments like git's option parser: `--where`, `--if-exists`
/// and `--if-missing` apply to the `--trailer` options that follow them (their `--no-` forms
/// go back to the configured behaviour), values may be glued with `=` or given as the next
/// argument, `--parse` is `--only-trailers --only-input --unfold`, and everything else (or
/// after `--`) is a file. Errors carry the usage text and mean exit 129.
// Used by `interpret-trailers`.
pub fn parse_interpret_trailers_args(args: &[String]) -> anyhow::Result<InterpretTrailersOptions> {
    let mut opts = InterpretTrailersOptions::default();
    let (mut place, mut if_exists, mut if_missing) = (TrailerWhere::Default, TrailerIfExists::Default, TrailerIfMissing::Default);

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if arg == "--" {
            opts.files.extend(args[i..].iter().cloned());
            break;
        }
        if !arg.starts_with("--") {
            if arg.starts_with('-') && arg.len() > 1 {
                anyhow::bail!("error: unknown switch `{}'\n{}", &arg[1..2], INTERPRET_TRAILERS_USAGE);
            }
            opts.files.push(arg.to_string());
            continue;
        }

        let (name, glued) = match arg[2..].split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (&arg[2..], None),
        };
        let value = |i: &mut usize| -> anyhow::Result<String> {
            if let Some(v) = &glued {
                return Ok(v.clone());
            }
            let v = args.get(*i).cloned().ok_or_else(|| anyhow::anyhow!("error: option `{}' requires a value\n", name))?;
            *i += 1;
            Ok(v)
        };
        // git exits 129 without a message for a placement or action it doesn't know.
        let bad_value = |_: ()| anyhow::anyhow!("");
        match name {
            "in-place" => opts.in_place = true,
            "no-in-place" => opts.in_place = false,
            "trim-empty" => opts.trim_empty = true,
            "no-trim-empty" => opts.trim_empty = false,
            "only-trailers" => opts.only_trailers = true,
            "no-only-trailers" => opts.only_trailers = false,
            "only-input" => opts.only_input = true,
            "no-only-input" => opts.only_input = false,
            "unfold" => opts.unfold = true,
            "no-unfold" => opts.unfold = false,
            "no-divider" => opts.no_divider = true,
            "divider" => opts.no_divider = false,
            "parse" => {
                opts.only_trailers = true;
                opts.only_input = true;
                opts.unfold = true;
            }
            "where" => place = parse_trailer_where(&value(&mut i)?).ok_or(()).map_err(bad_value)?,
            "no-where" => place = TrailerWhere::Default,
            "if-exists" => if_exists = parse_trailer_if_exists(&value(&mut i)?).ok_or(()).map_err(bad_value)?,
            "no-if-exists" => if_exists = TrailerIfExists::Default,
            "if-missing" => if_missing = parse_trailer_if_missing(&value(&mut i)?).ok_or(()).map_err(bad_value)?,
            "no-if-missing" => if_missing = TrailerIfMissing::Default,
            "trailer" => {
                let text = value(&mut i)?;
                opts.trailers.push(NewTrailer { text, place, if_exists, if_missing });
            }
            "no-trailer" => opts.trailers.clear(),
            other => anyhow::bail!("error: unknown option `{}'\n{}", other, INTERPRET_TRAILERS_USAGE),
        }
    }

    if opts.only_input && !opts.trailers.is_empty() {
        anyhow::bail!("fatal: --trailer with --only-input does not make sense\n\n{}", INTERPRET_TRAILERS_USAGE);
    }
    Ok(opts)
}

/// Reads the `trailer.*` configuration (global, then local) like git's `ensure_configured`:
/// `trailer.separators`, the default `trailer.where` / `ifexists` / `ifmissing`, and for each
/// `trailer.<token>.*` its `key`, `command`, `cmd`, `where`, `ifexists` and `ifmissing`.
/// Unknown values only produce a warning.
// Used by `interpret-trailers`.
pub fn read_trailer_config() -> TrailerConfig {
    let entries = read_all_config_entries();
    let trailer_entries: Vec<(&str, &str)> = entries.iter().filter_map(|(k, v)| Some((k.strip_prefix("trailer.")?, v.as_str()))).collect();
    let warn = |key: &str, value: &str| eprintln!("warning: unknown value '{}' for key 'trailer.{}'", value, key);

    let mut config = TrailerConfig {
        separators: ":".to_string(),
        default: TrailerConf { place: TrailerWhere::End, if_exists: TrailerIfExists::AddIfDifferentNeighbor, if_missing: TrailerIfMissing::Add, ..Default::default() },
        items: Vec::new(),
    };

    // Defaults first, so every per-token entry starts from them.
    for (key, value) in trailer_entries.iter().filter(|(k, _)| !k.contains('.')) {
        match *key {
            "where" => match parse_trailer_where(value) {
                Some(w) => config.default.place = w,
                None => warn(key, value),
            },
            "ifexists" => match parse_trailer_if_exists(value) {
                Some(e) => config.default.if_exists = e,
                None => warn(key, value),
            },
            "ifmissing" => match parse_trailer_if_missing(value) {
                Some(m) => config.default.if_missing = m,
                None => warn(key, value),
            },
            "separators" => config.separators = value.to_string(),
            _ => {}
        }
    }

    for (key, value) in &trailer_entries {
        let Some((name, variable)) = key.rsplit_once('.') else { continue };
        if !matches!(variable, "key" | "command" | "cmd" | "where" | "ifexists" | "ifmissing") {
            continue;
        }
        let index = match config.items.iter().position(|item| item.name.eq_ignore_ascii_case(name)) {
            Some(index) => index,
            None => {
                config.items.push(TrailerConf { name: name.to_string(), ..config.default.clone() });
                config.items.len() - 1
            }
        };
        let conf = &mut config.items[index];
        let duplicate = |already: bool| {
            if already {
                eprintln!("warning: more than one trailer.{}", key);
            }
        };
        match variable {
            "key" => {
                duplicate(conf.key.is_some());
                conf.key = Some(value.to_string());
            }
            "command" => {
                duplicate(conf.command.is_some());
                conf.command = Some(value.to_string());
            }
            "cmd" => {
                duplicate(conf.cmd.is_some());
                conf.cmd = Some(value.to_string());
            }
            "where" => match parse_trailer_where(value) {
                Some(w) => conf.place = w,
                None => warn(key, value),
            },
            "ifexists" => match parse_trailer_if_exists(value) {
                Some(e) => conf.if_exists = e,
                None => warn(key, value),
            },
            _ => match parse_trailer_if_missing(value) {
                Some(m) => conf.if_missing = m,
                None => warn(key, value),
            },
        }
    }
    config
}

/// Port of git's `process_trailers` for one message: finds the trailer block (the last
/// paragraph before any `---` divider, if it consists of trailers), parses it, applies the
/// new trailers from the command line and the configuration, and returns the rewritten
/// message. With `only_trailers` just the trailers are returned.
// Used by `interpret-trailers`.
pub fn process_trailers(message: &[u8], opts: &InterpretTrailersOptions, config: &TrailerConfig) -> Vec<u8> {
    let mut out = Vec::new();
    let separators = config.separators.as_bytes();

    let patch_start = if opts.no_divider { message.len() } else { find_patch_start(message) };
    let trailer_end = patch_start - ignore_non_trailer(message, patch_start);
    let trailer_start = find_trailer_start(message, trailer_end, config);

    // Group the block into trailers, attaching continuation lines to the trailer above.
    let mut raw_trailers: Vec<Vec<u8>> = Vec::new();
    let mut last_is_trailer = false;
    for line in message[trailer_start..trailer_end].split_inclusive(|&b| b == b'\n') {
        if last_is_trailer && line.first().is_some_and(|&b| is_space(b)) {
            raw_trailers.last_mut().expect("a trailer precedes its continuation").extend_from_slice(line);
            continue;
        }
        last_is_trailer = find_separator(line, separators).is_some_and(|p| p >= 1);
        raw_trailers.push(line.to_vec());
    }

    if !opts.only_trailers {
        out.extend_from_slice(&message[..trailer_start]);
        if !ends_with_blank_line(message, trailer_start) {
            out.push(b'\n');
        }
    }

    let mut items: Vec<TrailerItem> = Vec::new();
    for trailer in &raw_trailers {
        if trailer.first() == Some(&COMMENT_LINE_CHAR) {
            continue;
        }
        match find_separator(trailer, separators).filter(|p| *p >= 1) {
            Some(separator_pos) => {
                let (token, mut value, _) = parse_trailer(trailer, Some(separator_pos), config);
                if opts.unfold {
                    value = unfold_value(&value);
                }
                items.push(TrailerItem { token: Some(token), value });
            }
            None if !opts.only_trailers => {
                let line = trailer.strip_suffix(b"\n").unwrap_or(trailer);
                items.push(TrailerItem { token: None, value: line.to_vec() });
            }
            None => {}
        }
    }

    if !opts.only_input {
        for arg in command_line_trailer_args(&opts.trailers, config) {
            if let Some(arg) = apply_arg_if_exists(&mut items, arg) {
                apply_arg_if_missing(&mut items, arg);
            }
        }
    }

    for item in &items {
        if (!opts.trim_empty || !item.value.is_empty()) && (!opts.only_trailers || item.token.is_some()) {
            print_token_value(&mut out, item, separators);
        }
    }

    if !opts.only_trailers {
        out.extend_from_slice(&message[trailer_end..]);
    }
    out
}

/// git's `print_tok_val`: `token: value`, using the token's own final separator if it has
/// one, and the first configured separator otherwise; non-trailer lines print as they are.
// Used by `interpret-trailers` (internal to this file's helpers).
fn print_token_value(out: &mut Vec<u8>, item: &TrailerItem, separators: &[u8]) {
    let Some(token) = &item.token else {
        out.extend_from_slice(&item.value);
        out.push(b'\n');
        return;
    };
    let Some(&last) = token.iter().rev().find(|&&b| !is_space(b)) else { return };
    out.extend_from_slice(token);
    if !separators.contains(&last) {
        out.push(separators.first().copied().unwrap_or(b':'));
        out.push(b' ');
    }
    out.extend_from_slice(&item.value);
    out.push(b'\n');
}

/// Offset of the line after the one starting at `pos` (the buffer length on the last line).
// Used by `interpret-trailers` (internal to this file's helpers).
fn next_line(buf: &[u8], pos: usize) -> usize {
    buf[pos..].iter().position(|&b| b == b'\n').map(|p| pos + p + 1).unwrap_or(buf.len())
}

/// git's `last_line`: the start of the last line of `buf[..len]`, `None` when it is empty.
// Used by `interpret-trailers` (internal to this file's helpers).
fn last_line(buf: &[u8], len: usize) -> Option<usize> {
    match len {
        0 => None,
        1 => Some(0),
        _ => Some((0..=len - 2).rev().find(|&i| buf[i] == b'\n').map(|i| i + 1).unwrap_or(0)),
    }
}

/// git's `is_blank_line`: only whitespace up to the end of the line.
// Used by `interpret-trailers` (internal to this file's helpers).
fn is_blank_line(line: &[u8]) -> bool {
    line.iter().take_while(|&&b| b != b'\n').all(|&b| is_space(b))
}

/// git's `ends_with_blank_line`: the last line of `buf[..len]` is blank.
// Used by `interpret-trailers` (internal to this file's helpers).
fn ends_with_blank_line(buf: &[u8], len: usize) -> bool {
    last_line(buf, len).is_some_and(|start| is_blank_line(&buf[start..]))
}

/// git's `find_patch_start`: the offset of the first `---` line followed by whitespace (the
/// divider before a patch), or the message length.
// Used by `interpret-trailers` (internal to this file's helpers).
fn find_patch_start(message: &[u8]) -> usize {
    let mut pos = 0;
    while pos < message.len() {
        let line = &message[pos..];
        if line.starts_with(b"---") && line.get(3).is_some_and(|&b| is_space(b)) {
            return pos;
        }
        pos = next_line(message, pos);
    }
    message.len()
}

/// Port of git's `ignore_non_trailer`: how many bytes at the end of `buf[..len]` are not
/// part of the message proper (trailing comment and blank lines, an old-style `Conflicts:`
/// block, or everything from a scissors line on).
// Used by `interpret-trailers` (internal to this file's helpers).
fn ignore_non_trailer(buf: &[u8], len: usize) -> usize {
    let cutoff = if buf.starts_with(&CUT_LINE[1..]) {
        0
    } else {
        buf.windows(CUT_LINE.len()).position(|w| w == CUT_LINE).map(|p| p + 1).unwrap_or(len)
    };
    let (mut boc, mut bol, mut in_old_conflicts_block) = (0, 0, false);
    while bol < cutoff.min(len) {
        let next = next_line(&buf[..len], bol);
        if buf[bol] == COMMENT_LINE_CHAR || buf[bol] == b'\n' {
            if boc == 0 {
                boc = bol;
            }
        } else if buf[bol..].starts_with(b"Conflicts:\n") {
            in_old_conflicts_block = true;
            if boc == 0 {
                boc = bol;
            }
        } else if in_old_conflicts_block && buf[bol] == b'\t' {
            // A path name inside the conflicts block.
        } else if boc != 0 {
            boc = 0;
            in_old_conflicts_block = false;
        }
        bol = next;
    }
    if boc != 0 { len - boc } else { len - cutoff.min(len) }
}

/// git's `find_separator`: the position of the separator in `<token><optional blanks><separator>...`
/// (0 when the line starts with one), or `None` when the line is not of that form.
// Used by `interpret-trailers` (internal to this file's helpers).
fn find_separator(line: &[u8], separators: &[u8]) -> Option<usize> {
    let mut whitespace_found = false;
    for (i, &c) in line.iter().enumerate() {
        if separators.contains(&c) {
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

/// git's `token_len_without_separator`: the token's length without trailing punctuation.
// Used by `interpret-trailers` (internal to this file's helpers).
fn token_len_without_separator(token: &[u8]) -> usize {
    token.iter().rposition(|b| b.is_ascii_alphanumeric()).map(|p| p + 1).unwrap_or(0)
}

/// git's `token_matches_item`: the first `len` bytes of `token` are a prefix (any case) of
/// the configured name or key.
// Used by `interpret-trailers` (internal to this file's helpers).
fn token_matches_conf(token: &[u8], conf: &TrailerConf, len: usize) -> bool {
    equal_prefix_ignore_case(token, conf.name.as_bytes(), len) || conf.key.as_ref().is_some_and(|key| equal_prefix_ignore_case(token, key.as_bytes(), len))
}

/// Port of git's `find_trailer_start`: looking backwards from the end, the last paragraph
/// (after the title) is the trailer block if all its lines are trailers, or if it has a
/// git-generated or configured trailer and at least 25% trailer lines. Returns its offset,
/// or `len` when there is no trailer block.
// Used by `interpret-trailers` (internal to this file's helpers).
fn find_trailer_start(buf: &[u8], len: usize, config: &TrailerConfig) -> usize {
    let separators = config.separators.as_bytes();
    let mut s = 0;
    while s < len {
        if buf[s] != COMMENT_LINE_CHAR && is_blank_line(&buf[s..]) {
            break;
        }
        s = next_line(buf, s);
    }
    let end_of_title = s;

    let (mut only_spaces, mut recognized_prefix) = (true, false);
    let (mut trailer_lines, mut non_trailer_lines, mut possible_continuation_lines) = (0, 0, 0);
    let mut l = last_line(buf, len);
    while let Some(bol) = l.filter(|&bol| bol >= end_of_title) {
        l = last_line(buf, bol);
        let line = &buf[bol..];

        if line.first() == Some(&COMMENT_LINE_CHAR) {
            non_trailer_lines += possible_continuation_lines;
            possible_continuation_lines = 0;
            continue;
        }
        if is_blank_line(line) {
            if only_spaces {
                continue;
            }
            non_trailer_lines += possible_continuation_lines;
            if (recognized_prefix && trailer_lines * 3 >= non_trailer_lines) || (trailer_lines > 0 && non_trailer_lines == 0) {
                return next_line(buf, bol);
            }
            return len;
        }
        only_spaces = false;

        if GIT_GENERATED_PREFIXES.iter().any(|prefix| line.starts_with(prefix)) {
            trailer_lines += 1;
            possible_continuation_lines = 0;
            recognized_prefix = true;
            continue;
        }

        let this_line = &line[..line.iter().position(|&b| b == b'\n').unwrap_or(line.len())];
        match find_separator(this_line, separators).filter(|p| *p >= 1 && !is_space(line[0])) {
            Some(separator_pos) => {
                trailer_lines += 1;
                possible_continuation_lines = 0;
                if !recognized_prefix {
                    recognized_prefix = config.items.iter().any(|conf| token_matches_conf(line, conf, separator_pos));
                }
            }
            None if is_space(line[0]) => possible_continuation_lines += 1,
            None => {
                non_trailer_lines += 1 + possible_continuation_lines;
                possible_continuation_lines = 0;
            }
        }
    }
    len
}

/// Port of git's `parse_trailer`: splits `trailer` at the separator into trimmed token and
/// value (the whole text is the token without one), then, if the token is a prefix of a
/// configured trailer's name or key, switches to that trailer's settings and its `key`.
// Used by `interpret-trailers` (internal to this file's helpers).
fn parse_trailer(trailer: &[u8], separator_pos: Option<usize>, config: &TrailerConfig) -> (Vec<u8>, Vec<u8>, TrailerConf) {
    let (mut token, value) = match separator_pos {
        Some(pos) => (trimmed(&trailer[..pos]), trimmed(&trailer[pos + 1..])),
        None => (trimmed(trailer), Vec::new()),
    };
    let token_len = token_len_without_separator(&token);
    let mut conf = config.default.clone();
    if let Some(item) = config.items.iter().find(|item| token_matches_conf(&token, item, token_len)) {
        conf = item.clone();
        if let Some(key) = &item.key {
            token = key.as_bytes().to_vec();
        }
    }
    (token, value, conf)
}

/// git's `unfold_value`: joins a multi-line value into one line (each line break and the
/// indentation after it become a single space).
// Used by `interpret-trailers` (internal to this file's helpers).
fn unfold_value(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        let c = value[i];
        i += 1;
        if c == b'\n' {
            while i < value.len() && is_space(value[i]) {
                i += 1;
            }
            out.push(b' ');
        } else {
            out.push(c);
        }
    }
    trimmed(&out)
}

/// Port of git's `process_command_line_args`: one argument per configured trailer that has
/// a `command`, then one per `--trailer` (where `=` also works as separator). A `--trailer`
/// without a token is reported and skipped.
// Used by `interpret-trailers` (internal to this file's helpers).
fn command_line_trailer_args(new_trailers: &[NewTrailer], config: &TrailerConfig) -> Vec<TrailerArg> {
    let mut args = Vec::new();
    for item in config.items.iter().filter(|item| item.command.is_some()) {
        let token = item.key.clone().unwrap_or_else(|| item.name.clone());
        args.push(TrailerArg { token: token.into_bytes(), value: Vec::new(), conf: item.clone() });
    }

    let mut separators = b"=".to_vec();
    separators.extend_from_slice(config.separators.as_bytes());
    for new in new_trailers {
        let text = new.text.as_bytes();
        let separator_pos = find_separator(text, &separators);
        if separator_pos == Some(0) {
            eprintln!("error: empty trailer token in trailer '{}'", String::from_utf8_lossy(&trimmed(text)));
            continue;
        }
        let (token, value, mut conf) = parse_trailer(text, separator_pos, config);
        if new.place != TrailerWhere::Default {
            conf.place = new.place;
        }
        if new.if_exists != TrailerIfExists::Default {
            conf.if_exists = new.if_exists;
        }
        if new.if_missing != TrailerIfMissing::Default {
            conf.if_missing = new.if_missing;
        }
        args.push(TrailerArg { token, value, conf });
    }
    args
}

/// git's `after_or_end`: the new trailer goes after its anchor rather than before it.
// Used by `interpret-trailers` (internal to this file's helpers).
fn after_or_end(place: TrailerWhere) -> bool {
    matches!(place, TrailerWhere::After | TrailerWhere::End)
}

/// git's `same_token`: the shorter of the two tokens (ignoring trailing punctuation) is a
/// prefix of the other, any case. Non-trailer lines never match.
// Used by `interpret-trailers` (internal to this file's helpers).
fn same_token(item: &TrailerItem, arg: &TrailerArg) -> bool {
    let Some(token) = &item.token else { return false };
    let min_len = token_len_without_separator(token).min(token_len_without_separator(&arg.token));
    equal_prefix_ignore_case(token, &arg.token, min_len)
}

/// git's `same_trailer`: same token and same value, ignoring case.
// Used by `interpret-trailers` (internal to this file's helpers).
fn same_trailer(item: &TrailerItem, arg: &TrailerArg) -> bool {
    same_token(item, arg) && item.value.eq_ignore_ascii_case(&arg.value)
}

/// git's `check_if_different`: true unless the trailer at `index` (or, with `check_all`,
/// any trailer from there towards the side the new one is added on) equals the new one.
// Used by `interpret-trailers` (internal to this file's helpers).
fn check_if_different(items: &[TrailerItem], mut index: usize, arg: &TrailerArg, check_all: bool) -> bool {
    loop {
        if same_trailer(&items[index], arg) {
            return false;
        }
        let next = if after_or_end(arg.conf.place) { index.checked_sub(1) } else { Some(index + 1).filter(|&n| n < items.len()) };
        match next {
            Some(n) if check_all => index = n,
            _ => return true,
        }
    }
}

/// Port of git's `apply_command`: runs a trailer's `cmd` (with the value as its argument)
/// or `command` (with `$ARG` replaced by the value) through the shell and returns its
/// trimmed output, or an empty value after reporting a failure.
// Used by `interpret-trailers` (internal to this file's helpers).
fn apply_command(conf: &TrailerConf, arg: &[u8]) -> Vec<u8> {
    let arg = String::from_utf8_lossy(arg).into_owned();
    let (script, display, extra): (String, String, Vec<String>) = match (&conf.cmd, &conf.command) {
        (Some(cmd), _) => (format!("{} \"$@\"", cmd), cmd.clone(), vec![cmd.clone(), arg]),
        (None, Some(command)) => {
            let replaced = command.replacen("$ARG", &arg, 1);
            (replaced.clone(), replaced, Vec::new())
        }
        (None, None) => return Vec::new(),
    };
    let output = Command::new("sh").arg("-c").arg(&script).args(&extra).stdin(Stdio::null()).stderr(Stdio::inherit()).output();
    match output {
        Ok(output) if output.status.success() => trimmed(&output.stdout),
        _ => {
            eprintln!("error: running trailer command '{}' failed", display);
            Vec::new()
        }
    }
}

/// git's `apply_item_command`: replaces the new trailer's value with the output of its
/// configured command, passing the given value, or else the existing trailer's value.
// Used by `interpret-trailers` (internal to this file's helpers).
fn apply_item_command(existing: Option<&TrailerItem>, arg: &mut TrailerArg) {
    if arg.conf.command.is_none() && arg.conf.cmd.is_none() {
        return;
    }
    let input = if !arg.value.is_empty() { arg.value.clone() } else { existing.map(|item| item.value.clone()).unwrap_or_default() };
    arg.value = apply_command(&arg.conf, &input);
}

/// git's `add_arg_to_input_list`: inserts the new trailer right after (`after` / `end`) or
/// right before (`before` / `start`) the trailer at `on`. Returns where it went.
// Used by `interpret-trailers` (internal to this file's helpers).
fn add_arg_next_to(items: &mut Vec<TrailerItem>, on: usize, arg: TrailerArg) -> usize {
    let at = if after_or_end(arg.conf.place) { on + 1 } else { on };
    items.insert(at, TrailerItem { token: Some(arg.token), value: arg.value });
    at
}

/// Port of git's `find_same_and_apply_arg` + `apply_arg_if_exists`: looks for an existing
/// trailer with the same token (from the end for `after` / `end`, from the start otherwise)
/// and applies the new trailer's `ifExists` action next to it (or at the end/start of the
/// block). Gives the argument back when no trailer has that token.
// Used by `interpret-trailers` (internal to this file's helpers).
fn apply_arg_if_exists(items: &mut Vec<TrailerItem>, mut arg: TrailerArg) -> Option<TrailerArg> {
    let place = arg.conf.place;
    let backwards = after_or_end(place);
    let found = if backwards { (0..items.len()).rev().find(|&i| same_token(&items[i], &arg)) } else { (0..items.len()).find(|&i| same_token(&items[i], &arg)) };
    let Some(found) = found else { return Some(arg) };
    let on = if matches!(place, TrailerWhere::After | TrailerWhere::Before) {
        found
    } else if backwards {
        items.len() - 1
    } else {
        0
    };

    match arg.conf.if_exists {
        TrailerIfExists::DoNothing => {}
        TrailerIfExists::Replace => {
            apply_item_command(Some(&items[found]), &mut arg);
            let at = add_arg_next_to(items, on, arg);
            items.remove(if at <= found { found + 1 } else { found });
        }
        TrailerIfExists::Add => {
            apply_item_command(Some(&items[found]), &mut arg);
            add_arg_next_to(items, on, arg);
        }
        TrailerIfExists::AddIfDifferent => {
            apply_item_command(Some(&items[found]), &mut arg);
            if check_if_different(items, found, &arg, true) {
                add_arg_next_to(items, on, arg);
            }
        }
        TrailerIfExists::AddIfDifferentNeighbor | TrailerIfExists::Default => {
            apply_item_command(Some(&items[found]), &mut arg);
            if check_if_different(items, on, &arg, false) {
                add_arg_next_to(items, on, arg);
            }
        }
    }
    None
}

/// Port of git's `apply_arg_if_missing`: a trailer whose token isn't in the message yet is
/// appended (`after` / `end`) or prepended, unless its `ifMissing` action is `doNothing`.
// Used by `interpret-trailers` (internal to this file's helpers).
fn apply_arg_if_missing(items: &mut Vec<TrailerItem>, mut arg: TrailerArg) {
    if arg.conf.if_missing == TrailerIfMissing::DoNothing {
        return;
    }
    apply_item_command(None, &mut arg);
    let item = TrailerItem { token: Some(arg.token), value: arg.value };
    if after_or_end(arg.conf.place) {
        items.push(item);
    } else {
        items.insert(0, item);
    }
}

/// Replaces `file` with `content` the way git's `--in-place` does: the file must be a
/// regular, writable file, and the new content is written to a temporary file next to it
/// that is then renamed over it (keeping the original permissions).
// Used by `interpret-trailers --in-place`.
pub fn write_trailers_in_place(file: &str, content: &[u8]) -> anyhow::Result<()> {
    let meta = fs::metadata(file).map_err(|_| anyhow::anyhow!("fatal: could not stat {}: No such file or directory", file))?;
    if !meta.is_file() {
        anyhow::bail!("fatal: file {} is not a regular file", file);
    }
    if meta.permissions().readonly() {
        anyhow::bail!("fatal: file {} is not writable by user", file);
    }
    let path = Path::new(file);
    let temp = path.with_file_name(format!("git-interpret-trailers-{}", std::process::id()));
    fs::write(&temp, content).map_err(|e| anyhow::anyhow!("fatal: could not open temporary file: {}", e))?;
    let _ = fs::set_permissions(&temp, meta.permissions());
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        anyhow::bail!("fatal: could not rename temporary file to {}: {}", file, e);
    }
    Ok(())
}
