use std::fs;
use std::path::Path;

use crate::helpers::check_ref_format::check_refname_format;

/// Usage text of `refs`, printed for `-h` and usage errors.
pub const REFS_USAGE: &str = "usage: rgit refs list [<for-each-ref options>] [<pattern>...]\n   or: rgit refs exists <ref>\n   or: rgit refs verify [--strict] [--verbose]\n   or: rgit refs optimize [<pack-refs options>]\n\n";

/// True for a full object name: 40 hexadecimal digits.
// Used by `check_ref_content`.
fn is_hash(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Checks what a loose ref file holds, like git's ref fsck. Returns `(is_error, message)`
/// pairs: a symbolic ref must point at a well-formed name (`badReferentName`), a normal ref
/// must start with a full hash (`badRefContent`); a missing final newline
/// (`refMissingNewline`) and anything after the hash (`trailingRefContent`) are only notes.
// Used by `verify_refs`.
fn check_ref_content(content: &str) -> Vec<(bool, String)> {
    let mut found = Vec::new();
    if let Some(target) = content.strip_prefix("ref:") {
        let referent = target.trim_start();
        let name = referent.trim_end();
        if !check_refname_format(name, true, false) {
            found.push((true, format!("badReferentName: points to invalid refname '{}'", name)));
        } else if !referent.ends_with('\n') {
            found.push((false, "refMissingNewline: misses LF at the end".to_string()));
        } else if referent.len() != name.len() + 1 {
            found.push((false, format!("trailingRefContent: has trailing whitespaces or newlines: '{}'", referent[name.len()..].escape_default())));
        }
        return found;
    }
    if content.len() < 40 || !content.is_char_boundary(40) || !is_hash(&content[..40]) {
        found.push((true, format!("badRefContent: {}", content.trim_end())));
        return found;
    }
    match &content[40..] {
        "\n" => {}
        "" => found.push((false, "refMissingNewline: misses LF at the end".to_string())),
        rest if rest.starts_with(|c: char| c.is_ascii_whitespace()) => found.push((false, format!("trailingRefContent: has trailing garbage: '{}'", rest.escape_default()))),
        _ => found.push((true, format!("badRefContent: {}", content.trim_end()))),
    }
    found
}

/// Walks the loose ref files under `dir`, checking each name and content.
// Used by `verify_refs`.
fn verify_loose_refs(dir: &Path, prefix: &str, checked: &mut Vec<String>, found: &mut Vec<(bool, String)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = format!("{}/{}", prefix, entry.file_name().to_string_lossy());
        let path = entry.path();
        if path.is_dir() {
            verify_loose_refs(&path, &name, checked, found);
            continue;
        }
        if name.ends_with(".lock") {
            continue;
        }
        checked.push(name.clone());
        if !check_refname_format(&name, false, false) {
            found.push((true, format!("{}: badRefName: invalid refname format", name)));
        }
        match fs::read(&path) {
            Ok(bytes) => {
                for (is_error, message) in check_ref_content(&String::from_utf8_lossy(&bytes)) {
                    found.push((is_error, format!("{}: {}", name, message)));
                }
            }
            Err(e) => found.push((true, format!("{}: badRefContent: cannot read ref file: {}", name, e))),
        }
    }
}

/// Checks `packed-refs`: every line must be `<hash> <refname>` (or a `^<hash>` peel line after
/// one, or the `#` header) with a well-formed name, and names must be sorted and unique.
// Used by `verify_refs`.
fn verify_packed_refs(checked: &mut Vec<String>, found: &mut Vec<(bool, String)>) {
    let Ok(packed) = fs::read_to_string(crate::repo::path(".git/packed-refs")) else { return };
    checked.push("packed-refs".to_string());
    let mut previous: Option<String> = None;
    for (index, line) in packed.lines().enumerate() {
        let at = format!("packed-refs line {}", index + 1);
        if line.starts_with('#') {
            if index != 0 {
                found.push((true, format!("{}: badPackedRefHeader: header not at the start of the file", at)));
            }
            continue;
        }
        if let Some(peeled) = line.strip_prefix('^') {
            if !is_hash(peeled) || previous.is_none() {
                found.push((true, format!("{}: badPackedRefEntry: '{}' has invalid peeled oid", at, line)));
            }
            continue;
        }
        match line.split_once(' ') {
            Some((hash, name)) if is_hash(hash) => {
                if !check_refname_format(name, false, false) {
                    found.push((true, format!("{}: badRefName: entry has bad refname '{}'", at, name)));
                }
                if previous.as_deref().is_some_and(|p| p >= name) {
                    found.push((true, format!("{}: packedRefUnsorted: refname '{}' is not sorted or is repeated", at, name)));
                }
                previous = Some(name.to_string());
            }
            _ => found.push((true, format!("{}: badPackedRefEntry: '{}' has invalid oid or no refname", at, line))),
        }
    }
}

/// Checks the ref database for consistency: every loose ref under `.git/refs` (in the main
/// repository, shared by all worktrees) and every `packed-refs` line. Returns the names that
/// were checked and the problems as `(is_error, message)`; notes count as errors when
/// `strict`.
// Used by `refs verify`.
pub fn verify_refs(strict: bool) -> (Vec<String>, Vec<(bool, String)>) {
    let mut checked = Vec::new();
    let mut found = Vec::new();
    verify_loose_refs(&crate::repo::path(".git/refs"), "refs", &mut checked, &mut found);
    verify_packed_refs(&mut checked, &mut found);
    if strict {
        for problem in &mut found {
            problem.0 = true;
        }
    }
    (checked, found)
}
