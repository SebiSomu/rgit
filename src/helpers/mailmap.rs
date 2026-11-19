use std::fs;
use std::path::Path;
use crate::core::config::get_config_value;
use crate::core::object_store::read_object;
use crate::core::rev_parse::resolve_rev;
use crate::objects::{CheckMailmapOptions, Mailmap, MailmapEntry, MailmapInfo};

/// git's usage text for `check-mailmap`, printed for `-h` and after option errors (exit 129).
pub const CHECK_MAILMAP_USAGE: &str = "usage: rgit check-mailmap [<options>] <contact>...

    --[no-]stdin          also read contacts from stdin
    --[no-]mailmap-file <file>
                          read additional mailmap entries from file
    --[no-]mailmap-blob <blob>
                          read additional mailmap entries from blob
";

/// Parses `check-mailmap` arguments: `--stdin`, `--mailmap-file <file>`, `--mailmap-blob
/// <blob>` (values glued with `=` or as the next argument) and the contacts. Errors carry
/// the usage text and mean exit 129.
// Used by `check-mailmap`.
pub fn parse_check_mailmap_args(args: &[String]) -> anyhow::Result<CheckMailmapOptions> {
    let mut opts = CheckMailmapOptions::default();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if arg == "--" {
            opts.contacts.extend(args[i..].iter().cloned());
            break;
        }
        if !arg.starts_with("--") {
            if arg.starts_with('-') && arg.len() > 1 {
                anyhow::bail!("error: unknown switch `{}'\n{}", &arg[1..2], CHECK_MAILMAP_USAGE);
            }
            opts.contacts.push(arg.to_string());
            continue;
        }
        let (name, glued) = match arg[2..].split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (&arg[2..], None),
        };
        let mut value = || -> anyhow::Result<String> {
            if let Some(v) = &glued {
                return Ok(v.clone());
            }
            let v = args.get(i).cloned().ok_or_else(|| anyhow::anyhow!("error: option `{}' requires a value\n", name))?;
            i += 1;
            Ok(v)
        };
        match name {
            "stdin" => opts.stdin = true,
            "no-stdin" => opts.stdin = false,
            "mailmap-file" => opts.mailmap_file = Some(value()?),
            "no-mailmap-file" => opts.mailmap_file = None,
            "mailmap-blob" => opts.mailmap_blob = Some(value()?),
            "no-mailmap-blob" => opts.mailmap_blob = None,
            other => anyhow::bail!("error: unknown option `{}'\n{}", other, CHECK_MAILMAP_USAGE),
        }
    }
    Ok(opts)
}

/// git's `parse_name_and_email`: reads `[name] <email>` from the start of `buffer` and
/// returns the name (if any), the email, and what follows the `>` (if anything).
// Used by `add_mailmap_line`.
fn parse_name_and_email(buffer: &str, allow_empty_email: bool) -> Option<(Option<String>, String, Option<&str>)> {
    let left = buffer.find('<')?;
    let right = left + 1 + buffer[left + 1..].find('>')?;
    if !allow_empty_email && left + 1 == right {
        return None;
    }
    let name = buffer[..left].trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r'));
    let rest = &buffer[right + 1..];
    Some(((!name.is_empty()).then(|| name.to_string()), buffer[left + 1..right].to_string(), (!rest.is_empty()).then_some(rest)))
}

/// Port of git's `read_mailmap_line` + `add_mapping`: adds one `.mailmap` line to the map.
/// The four forms are `Proper Name <commit@email>`, `<proper@email> <commit@email>`,
/// `Proper Name <proper@email> <commit@email>` and `Proper Name <proper@email> Commit Name
/// <commit@email>`; `#` starts a comment. A later line for the same key replaces an earlier one.
// Used by `read_mailmap` (and so by `check-mailmap` and `request-pull`).
pub fn add_mailmap_line(map: &mut Mailmap, line: &str) {
    if line.starts_with('#') {
        return;
    }
    let Some((name1, email1, rest)) = parse_name_and_email(line, false) else { return };
    let (name2, email2) = match rest.and_then(|rest| parse_name_and_email(rest, true)) {
        Some((name2, email2, _)) => (name2, Some(email2)),
        None => (None, None),
    };

    // With one address it is the commit email and only the name is corrected.
    let (new_name, new_email, old_name, old_email) = match email2 {
        Some(old_email) => (name1, Some(email1), name2, old_email),
        None => (name1, None, None, email1),
    };

    let index = match map.entries.iter().position(|e| e.old_email.eq_ignore_ascii_case(&old_email)) {
        Some(index) => index,
        None => {
            map.entries.push(MailmapEntry { old_email, ..Default::default() });
            map.entries.len() - 1
        }
    };
    let entry = &mut map.entries[index];
    match old_name {
        None => {
            if new_name.is_some() {
                entry.simple.name = new_name;
            }
            if new_email.is_some() {
                entry.simple.email = new_email;
            }
        }
        Some(old_name) => {
            let info = MailmapInfo { name: new_name, email: new_email };
            match entry.by_name.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(&old_name)) {
                Some(existing) => existing.1 = info,
                None => entry.by_name.push((old_name, info)),
            }
        }
    }
}

/// Reads the repository's mailmap like git's `read_mailmap`: `.mailmap` at the top of the
/// working tree, then the blob named by `mailmap.blob`, then the file named by
/// `mailmap.file`, and finally the extra blob and file given on the command line. Missing
/// sources are skipped; a blob name that isn't a blob is reported.
// Used by `check-mailmap` and `request-pull`.
pub fn read_mailmap(extra_file: Option<&str>, extra_blob: Option<&str>) -> Mailmap {
    let mut map = Mailmap::default();
    let config = |key: &str| get_config_value(key).ok().flatten().filter(|v| !v.is_empty());

    read_mailmap_file(&mut map, ".mailmap");
    if let Some(blob) = config("mailmap.blob") {
        read_mailmap_blob(&mut map, &blob);
    }
    if let Some(file) = config("mailmap.file") {
        read_mailmap_file(&mut map, &file);
    }
    if let Some(blob) = extra_blob {
        read_mailmap_blob(&mut map, blob);
    }
    if let Some(file) = extra_file {
        read_mailmap_file(&mut map, file);
    }
    map
}

/// Adds every line of a mailmap file to the map; a missing file is not an error.
// Used by `read_mailmap`.
fn read_mailmap_file(map: &mut Mailmap, path: &str) {
    match fs::read(Path::new(path)) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).split_inclusive('\n').for_each(|line| add_mailmap_line(map, line)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("error: unable to open mailmap at {}: {}", path, e),
    }
}

/// Adds every line of a mailmap blob (any revision expression, e.g. `HEAD:.mailmap`) to the
/// map; a name that doesn't resolve is skipped silently, like git.
// Used by `read_mailmap`.
fn read_mailmap_blob(map: &mut Mailmap, name: &str) {
    let Ok(hash) = resolve_rev(name) else { return };
    match read_object(&hash) {
        Ok((kind, content)) if kind == "blob" => String::from_utf8_lossy(&content).split_inclusive('\n').for_each(|line| add_mailmap_line(map, line)),
        Ok(_) => eprintln!("error: mailmap is not a blob: {}", name),
        Err(_) => eprintln!("error: unable to read mailmap object at {}", name),
    }
}

/// Port of git's `map_user`: looks the email up (ignoring case), prefers an entry for this
/// exact name over the email-only one, and returns the corrected `(name, email)`; parts the
/// mailmap doesn't mention stay as they were.
// Used by `check-mailmap` and `request-pull`.
pub fn map_user(map: &Mailmap, name: &str, email: &str) -> (String, String) {
    let Some(entry) = map.entries.iter().find(|e| e.old_email.eq_ignore_ascii_case(email)) else {
        return (name.to_string(), email.to_string());
    };
    let info = entry.by_name.iter().find(|(old_name, _)| old_name.eq_ignore_ascii_case(name)).map(|(_, info)| info).unwrap_or(&entry.simple);
    (info.name.clone().unwrap_or_else(|| name.to_string()), info.email.clone().unwrap_or_else(|| email.to_string()))
}

/// Splits a contact like git's `split_ident_line`: `Name <email>` gives the name (without
/// the blanks before `<`) and the email up to the first `>`. `None` when there is no
/// `<...>` part.
// Used by `check-mailmap`.
pub fn split_contact(contact: &str) -> Option<(String, String)> {
    let open = contact.find('<')?;
    let close = open + 1 + contact[open + 1..].find('>')?;
    let name = contact[..open].trim_end_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r'));
    Some((name.to_string(), contact[open + 1..close].to_string()))
}
