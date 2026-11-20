//! Line-ending conversion between the working tree and the object store, the way git does
//! it with `core.autocrlf`, `core.eol` and the `text`/`eol` attributes in `.gitattributes`.
//!
//! `clean` runs on content read from the working tree before it is hashed or stored (add,
//! status, diff, stash): with conversion on, CRLF becomes LF. `smudge` runs on content written
//! to the working tree (checkout, switch, reset, restore, merge): with conversion on and CRLF
//! as the checkout line ending, LF becomes CRLF. Binary files are never touched.

pub mod types;

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::core::eol::types::{Action, AttrLine, AutoCrlf, EolAttr, Settings, TextAttr};
use crate::core::wildmatch::{wildmatch, WM_PATHNAME};

/// Combines the settings with a path's attributes the way git's `convert.c` does.
pub fn action_for(settings: &Settings, text: TextAttr, eol: EolAttr) -> Action {
    if text == TextAttr::Unset {
        return Action::Binary;
    }
    let auto = match text {
        TextAttr::Auto => true,
        TextAttr::Set => false,
        TextAttr::Unspecified if eol != EolAttr::Unspecified => false,
        TextAttr::Unspecified => match settings.autocrlf {
            AutoCrlf::True => return Action::AutoCrlf,
            AutoCrlf::Input => return Action::AutoInput,
            AutoCrlf::False => return Action::Binary,
        },
        TextAttr::Unset => unreachable!(),
    };
    let crlf = match eol {
        EolAttr::Lf => false,
        EolAttr::Crlf => true,
        EolAttr::Unspecified => match settings.autocrlf {
            AutoCrlf::True => true,
            AutoCrlf::Input => false,
            AutoCrlf::False => settings.eol_crlf,
        },
    };
    match (auto, crlf) {
        (true, true) => Action::AutoCrlf,
        (true, false) => Action::AutoInput,
        (false, true) => Action::TextCrlf,
        (false, false) => Action::TextInput,
    }
}

/// Git's binary test: a NUL byte or a lone CR (one not followed by LF) means binary.
pub fn looks_binary(data: &[u8]) -> bool {
    data.iter().enumerate().any(|(i, &b)| b == 0 || (b == b'\r' && data.get(i + 1) != Some(&b'\n')))
}

/// Replaces every CRLF with LF; a lone CR stays.
pub fn crlf_to_lf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// Replaces every LF not already preceded by CR with CRLF.
pub fn lf_to_crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 16);
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' && (i == 0 || data[i - 1] != b'\r') {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

/// Worktree -> repository conversion for a known action. `index_has_cr` says whether the
/// version of the path already in the index contains CR; in the auto modes such a file is left
/// alone, so a file committed with CRLF does not show as modified (git's "safe autocrlf").
pub fn clean_with(action: Action, data: Vec<u8>, index_has_cr: impl FnOnce() -> bool) -> Vec<u8> {
    match action {
        Action::Binary => data,
        Action::TextInput | Action::TextCrlf => {
            if data.contains(&b'\r') { crlf_to_lf(&data) } else { data }
        }
        Action::AutoInput | Action::AutoCrlf => {
            if !data.contains(&b'\r') || looks_binary(&data) || index_has_cr() {
                data
            } else {
                crlf_to_lf(&data)
            }
        }
    }
}

/// Repository -> worktree conversion for a known action.
pub fn smudge_with(action: Action, data: &[u8]) -> Cow<'_, [u8]> {
    let convert = match action {
        Action::TextCrlf => data.contains(&b'\n'),
        Action::AutoCrlf => data.contains(&b'\n') && !data.contains(&b'\r') && !looks_binary(data),
        _ => false,
    };
    if convert { Cow::Owned(lf_to_crlf(data)) } else { Cow::Borrowed(data) }
}

/// Converts content read from the working tree at `path` (relative to the repository root)
/// into what is stored in the repository.
pub fn clean(path: &str, data: Vec<u8>) -> Vec<u8> {
    let action = path_action(path);
    clean_with(action, data, || index_blob_has_cr(path))
}

/// Converts content from the repository into what is written to the working tree at `path`.
pub fn smudge<'a>(path: &str, data: &'a [u8]) -> Cow<'a, [u8]> {
    smudge_with(path_action(path), data)
}

/// Reads a working-tree file and applies `clean`. `path` is relative to the repository root.
pub fn read_clean(path: &str) -> std::io::Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    Ok(clean(path, data))
}

/// The action for `path` with the current repository's settings and attributes.
pub fn path_action(path: &str) -> Action {
    let path = path.replace('\\', "/");
    let path = path.trim_start_matches("./");
    with_state(|state| {
        let (text, eol) = state.attributes(path);
        action_for(&state.settings, text, eol)
    })
}

// ---------------------------------------------------------------------------------------------
// Repository state: settings, attribute files and the index, cached per working directory.

struct State {
    settings: Settings,
    /// Parsed `.gitattributes` by directory ("" for the root).
    attr_files: HashMap<String, Vec<AttrLine>>,
    info_attrs: Vec<AttrLine>,
    /// Stage-0 index entries (path -> blob id), read on first use.
    index: Option<HashMap<String, String>>,
}

static STATE: Mutex<Option<(PathBuf, State)>> = Mutex::new(None);

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.as_ref().map(|(dir, _)| dir != &cwd).unwrap_or(true) {
        let info_attrs = std::fs::read_to_string(crate::core::repo::path(".git/info/attributes"))
            .map(|t| parse_attributes(&t))
            .unwrap_or_default();
        *guard = Some((cwd, State { settings: read_settings(), attr_files: HashMap::new(), info_attrs, index: None }));
    }
    f(&mut guard.as_mut().unwrap().1)
}

/// Forgets cached settings, attributes and index; for callers that change them mid-command.
pub fn reset_cache() {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

impl State {
    fn attributes(&mut self, path: &str) -> (TextAttr, EolAttr) {
        let mut text = TextAttr::Unspecified;
        let mut eol = EolAttr::Unspecified;
        let mut apply = |lines: &[AttrLine], base: &str| {
            let rel = if base.is_empty() { path } else { &path[base.len() + 1..] };
            for line in lines {
                if pattern_matches(&line.pattern, rel) {
                    if let Some(t) = line.text { text = t; }
                    if let Some(e) = line.eol { eol = e; }
                }
            }
        };

        let mut dirs = vec![String::new()];
        let mut prefix = String::new();
        let parts: Vec<&str> = path.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !prefix.is_empty() { prefix.push('/'); }
            prefix.push_str(part);
            dirs.push(prefix.clone());
        }
        for dir in dirs {
            let lines = self.attr_files.entry(dir.clone()).or_insert_with(|| {
                let file = if dir.is_empty() { PathBuf::from(".gitattributes") } else { Path::new(&dir).join(".gitattributes") };
                std::fs::read_to_string(file).map(|t| parse_attributes(&t)).unwrap_or_default()
            }).clone();
            apply(&lines, &dir);
        }
        let info = self.info_attrs.clone();
        apply(&info, "");
        (text, eol)
    }
}

/// Matches a `.gitattributes` pattern against a path relative to the file's directory: a
/// pattern without `/` matches the last path component at any depth; otherwise it matches
/// the whole relative path.
fn pattern_matches(pattern: &str, rel: &str) -> bool {
    let pattern = pattern.strip_suffix('/').unwrap_or(pattern);
    if pattern.contains('/') {
        wildmatch(pattern.trim_start_matches('/'), rel, WM_PATHNAME)
    } else {
        let base = rel.rsplit('/').next().unwrap_or(rel);
        wildmatch(pattern, base, WM_PATHNAME)
    }
}

/// Parses the lines of an attributes file, keeping only the attributes that affect line
/// endings (`text`, `eol`, `binary` and the old `crlf` spellings).
fn parse_attributes(text: &str) -> Vec<AttrLine> {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(pattern) = parts.next() else { continue };
        let mut entry = AttrLine { pattern: pattern.to_string(), text: None, eol: None };
        for attr in parts {
            match attr {
                "text" | "crlf" => entry.text = Some(TextAttr::Set),
                "-text" | "-crlf" | "binary" => entry.text = Some(TextAttr::Unset),
                "!text" | "!crlf" => entry.text = Some(TextAttr::Unspecified),
                "text=auto" => entry.text = Some(TextAttr::Auto),
                "eol=lf" | "crlf=input" => entry.eol = Some(EolAttr::Lf),
                "eol=crlf" => entry.eol = Some(EolAttr::Crlf),
                "!eol" => entry.eol = Some(EolAttr::Unspecified),
                _ => {}
            }
        }
        if entry.text.is_some() || entry.eol.is_some() {
            lines.push(entry);
        }
    }
    lines
}

/// Reads `core.autocrlf` and `core.eol`. rgit's own config comes first (the repository's
/// `.git/config`, then `~/.rgitconfig`); after that the files git itself reads (`~/.gitconfig`,
/// then the system gitconfig) so rgit converts the same way git does on this machine. Git for
/// Windows sets `core.autocrlf=true` in its system config. `GIT_CONFIG_NOSYSTEM` skips the
/// system file, as in git.
fn read_settings() -> Settings {
    let lookup = |key: &str| -> Option<String> {
        if let Ok(Some(v)) = crate::core::config::get_config_value(key) {
            return Some(v);
        }
        git_config_files().iter().find_map(|f| crate::core::config::get_from(f, key).ok().flatten())
    };
    let autocrlf = match lookup("core.autocrlf").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("true") | Some("yes") | Some("on") | Some("1") => AutoCrlf::True,
        Some("input") => AutoCrlf::Input,
        _ => AutoCrlf::False,
    };
    let eol_crlf = match lookup("core.eol").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("crlf") => true,
        Some("lf") => false,
        _ => cfg!(windows),
    };
    Settings { autocrlf, eol_crlf }
}

/// Git's own global and system config files, most specific first.
fn git_config_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    if let Some(home) = home {
        files.push(PathBuf::from(home).join(".gitconfig"));
    }
    if std::env::var_os("GIT_CONFIG_NOSYSTEM").is_none() {
        if let Some(system) = std::env::var_os("GIT_CONFIG_SYSTEM") {
            files.push(PathBuf::from(system));
        } else if cfg!(windows) {
            for var in ["ProgramW6432", "ProgramFiles"] {
                if let Some(dir) = std::env::var_os(var) {
                    files.push(PathBuf::from(dir).join("Git").join("etc").join("gitconfig"));
                }
            }
        } else {
            files.push(PathBuf::from("/etc/gitconfig"));
        }
    }
    files
}

/// Whether the stage-0 index version of `path` contains a CR.
fn index_blob_has_cr(path: &str) -> bool {
    let hash = with_state(|state| {
        let index = state.index.get_or_insert_with(|| {
            crate::core::index::read_index()
                .unwrap_or_default()
                .into_iter()
                .filter(|e| e.stage == 0)
                .map(|e| (e.path.clone(), hex::encode(e.hash)))
                .collect()
        });
        index.get(&path.replace('\\', "/")).cloned()
    });
    match hash {
        Some(h) => crate::core::object_store::read_object(&h).map(|(_, data)| data.contains(&b'\r')).unwrap_or(false),
        None => false,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/core/eol.rs"]
mod tests;
