use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const LOCAL_CONFIG_PATH: &str = ".git/config";
const DEFAULT_NAME: &str = "rgit";
const DEFAULT_EMAIL: &str = "rgit@example.com";

pub fn global_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".rgitconfig"))
}

fn is_valid_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn split_key(key: &str) -> Option<(String, Option<String>, String)> {
    let first = key.find('.')?;
    let last = key.rfind('.')?;
    let section = &key[..first];
    let name = &key[last + 1..];
    if !is_valid_ident(section) || !is_valid_ident(name) {
        return None;
    }
    let sub = if first == last {
        None
    } else {
        let s = &key[first + 1..last];
        if s.is_empty() || s.contains('\n') || s.contains('"') {
            return None;
        }
        Some(s.to_string())
    };
    Some((section.to_string(), sub, name.to_string()))
}

pub fn normalize_key(key: &str) -> Result<String> {
    let (section, sub, name) = split_key(key).ok_or_else(|| {
        anyhow::anyhow!("error: invalid key '{}' (expected section.name or section.subsection.name)", key)
    })?;
    let section = section.to_lowercase();
    let name = name.to_lowercase();
    Ok(match sub {
        Some(s) => format!("{}.{}.{}", section, s, name),
        None => format!("{}.{}", section, name),
    })
}

fn unescape_char(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        other => other,
    }
}

fn parse_header(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?;
    let end = inner.rfind(']')?;
    let inner = inner[..end].trim();

    if let Some(q) = inner.find('"') {
        let section = inner[..q].trim().to_lowercase();
        let rest = inner[q + 1..].strip_suffix('"')?;
        let mut sub = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    sub.push(n);
                }
            } else {
                sub.push(c);
            }
        }
        Some(format!("{}.{}", section, sub))
    } else {
        Some(inner.to_lowercase())
    }
}

fn parse_value(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(n) = chars.next() {
                        out.push(unescape_char(n));
                    }
                }
                '"' => break,
                c => out.push(c),
            }
        }
        return out;
    }
    let cut = raw.find(|c| c == '#' || c == ';').unwrap_or(raw.len());
    raw[..cut].trim().to_string()
}

fn format_value(value: &str) -> String {
    let needs_quotes = value.starts_with(char::is_whitespace)
        || value.ends_with(char::is_whitespace)
        || value.chars().any(|c| matches!(c, '#' | ';' | '"' | '\\' | '\n' | '\t'));
    if !needs_quotes {
        return value.to_string();
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn read_entries_from(path: &Path) -> Result<Vec<(String, String)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let content = content.trim_start_matches('\u{feff}');

    let mut prefix: Option<String> = None;
    let mut out = Vec::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') {
            prefix = parse_header(line);
            continue;
        }
        let Some(prefix) = &prefix else { continue };

        let (key, value) = match line.split_once('=') {
            Some((k, v)) => (k.trim().to_lowercase(), parse_value(v)),
            None => {
                let k = line.split(|c| c == '#' || c == ';').next().unwrap_or("").trim().to_lowercase();
                (k, "true".to_string())
            }
        };
        if key.is_empty() {
            continue;
        }
        out.push((format!("{}.{}", prefix, key), value));
    }

    Ok(out)
}

pub fn write_entries_to(path: &Path, entries: &[(String, String)]) -> Result<()> {
    let mut groups: Vec<((String, Option<String>), Vec<(String, String)>)> = Vec::new();

    for (key, value) in entries {
        let Some((section, sub, name)) = split_key(key) else { continue };
        let header = (section, sub);
        if let Some(i) = groups.iter().position(|(h, _)| *h == header) {
            groups[i].1.push((name, value.clone()));
        } else {
            groups.push((header, vec![(name, value.clone())]));
        }
    }

    let mut out = String::new();
    for ((section, sub), items) in &groups {
        match sub {
            Some(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                out.push_str(&format!("[{} \"{}\"]\n", section, escaped));
            }
            None => out.push_str(&format!("[{}]\n", section)),
        }
        for (name, value) in items {
            out.push_str(&format!("\t{} = {}\n", name, format_value(value)));
        }
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
    }
    fs::write(path, out).with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn set_in(path: &Path, key: &str, value: &str) -> Result<()> {
    let key = normalize_key(key)?;
    let mut entries = read_entries_from(path)?;

    let mut seen = false;
    entries.retain(|(k, _)| {
        if *k == key {
            if seen {
                return false;
            }
            seen = true;
        }
        true
    });

    if let Some(entry) = entries.iter_mut().find(|(k, _)| *k == key) {
        entry.1 = value.to_string();
    } else {
        entries.push((key, value.to_string()));
    }

    write_entries_to(path, &entries)
}

pub fn unset_in(path: &Path, key: &str) -> Result<bool> {
    let key = normalize_key(key)?;
    let mut entries = read_entries_from(path)?;
    let before = entries.len();
    entries.retain(|(k, _)| *k != key);
    if entries.len() == before {
        return Ok(false);
    }
    write_entries_to(path, &entries)?;
    Ok(true)
}

pub fn get_from(path: &Path, key: &str) -> Result<Option<String>> {
    let key = normalize_key(key)?;
    Ok(read_entries_from(path)?
        .into_iter()
        .rev()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v))
}

pub fn get_config_value(key: &str) -> Result<Option<String>> {
    if let Some(v) = get_from(&crate::repo::path(LOCAL_CONFIG_PATH), key)? {
        return Ok(Some(v));
    }
    match global_config_path() {
        Some(global) => get_from(&global, key),
        None => Ok(None),
    }
}

fn sanitize_ident(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '<' | '>' | '\n' | '\r' | '\0'))
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn author_identity() -> Result<String> {
    let pick = |key: &str, default: &str| -> Result<String> {
        Ok(get_config_value(key)?
            .map(|v| sanitize_ident(&v))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default.to_string()))
    };
    let name = pick("user.name", DEFAULT_NAME)?;
    let email = pick("user.email", DEFAULT_EMAIL)?;
    Ok(format!("{} <{}>", name, email))
}