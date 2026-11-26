//! Engines of `rgit ls-remote`: argument parsing, the ref lists of a local repository and of
//! a bundle (smart HTTP comes from `remote::smart_http`), and git's filters and output format.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use anyhow::Result;

use crate::remote::ls_remote::types::{ListedRef, LsRemoteOptions};
use crate::remote::smart_http::types::RemoteRef;

/// git's usage text for `ls-remote`.
pub const USAGE: &str = "usage: rgit ls-remote [--heads] [--tags] [--refs] [--upload-pack=<exec>]\n                      [-q | --quiet] [--exit-code] [--get-url] [--symref]\n                      [<repository> [<patterns>...]]";

/// Parses `ls-remote`'s arguments. `Err` holds the message for a usage error (exit 129). A
/// lone `-h` is the help request, as in git; anywhere else it means `--heads`.
// Used by `ls-remote`.
pub fn parse_ls_remote_args(args: &[String]) -> Result<LsRemoteOptions, String> {
    if args.len() == 1 && args[0] == "-h" {
        return Err(USAGE.to_string());
    }
    let mut options = LsRemoteOptions::default();
    let mut positional = Vec::new();
    let mut only_positional = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if only_positional || !arg.starts_with('-') || arg == "-" {
            positional.push(arg.to_string());
            continue;
        }
        match arg {
            "--" => only_positional = true,
            "-h" | "--heads" | "-b" | "--branches" => options.heads = true,
            "-t" | "--tags" => options.tags = true,
            "--refs" => options.refs_only = true,
            "--symref" => options.symref = true,
            "-q" | "--quiet" => options.quiet = true,
            "--exit-code" => options.exit_code = true,
            "--get-url" => options.get_url = true,
            // Only the local transport would run another program; rgit reads repositories itself.
            "--upload-pack" | "--exec" => i += 1,
            _ if arg.starts_with("--upload-pack=") || arg.starts_with("--exec=") => {}
            _ if arg.starts_with("-") && !arg.starts_with("--") && arg.len() > 2 => {
                // Bundled short flags: -qt.
                for flag in arg[1..].chars() {
                    match flag {
                        'h' | 'b' => options.heads = true,
                        't' => options.tags = true,
                        'q' => options.quiet = true,
                        other => return Err(format!("error: unknown switch `{}'\n{}", other, USAGE)),
                    }
                }
            }
            _ if arg.starts_with("--") => return Err(format!("error: unknown option `{}'\n{}", &arg[2..], USAGE)),
            _ => return Err(format!("error: unknown switch `{}'\n{}", &arg[1..], USAGE)),
        }
    }
    let mut positional = positional.into_iter();
    options.repository = positional.next();
    options.patterns = positional.collect();
    Ok(options)
}

/// The refs of a repository on this machine, as `git upload-pack` advertises them: `HEAD`
/// first, then every ref under `refs/` (loose and packed) sorted by name, with symbolic refs'
/// targets and annotated tags peeled. An unborn HEAD is left out, as git's ls-remote does.
// Used by `ls-remote` over the local transport.
pub fn local_refs(git_dir: &Path) -> Result<Vec<RemoteRef>> {
    // name -> raw value: an object id, or "ref: <target>" for a symbolic ref.
    let mut raw: BTreeMap<String, String> = BTreeMap::new();
    let mut packed_peeled: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(text) = fs::read_to_string(git_dir.join("packed-refs")) {
        let mut last: Option<String> = None;
        for line in text.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            if let Some(peeled) = line.strip_prefix('^') {
                if let Some(name) = &last {
                    packed_peeled.insert(name.clone(), peeled.trim().to_string());
                }
                continue;
            }
            if let Some((oid, name)) = line.split_once(' ') {
                raw.insert(name.trim().to_string(), oid.trim().to_string());
                last = Some(name.trim().to_string());
            }
        }
    }
    collect_loose_refs(&git_dir.join("refs"), "refs", &mut raw, &mut packed_peeled);

    let objects_dir = git_dir.join("objects");
    let mut refs = Vec::new();
    let head = fs::read_to_string(git_dir.join("HEAD")).unwrap_or_default().trim().to_string();
    let head_value = (!head.is_empty()).then_some(head);
    for (name, value) in head_value.iter().map(|v| ("HEAD".to_string(), v.clone())).chain(raw.clone()) {
        let symref_target = value.strip_prefix("ref: ").map(|t| t.trim().to_string());
        let Some(oid) = resolve_value(&value, &raw) else { continue };
        let peeled = packed_peeled.get(&name).cloned().or_else(|| peel(&objects_dir, &oid));
        refs.push(RemoteRef { name, oid: Some(oid), symref_target, peeled });
    }
    Ok(refs)
}

/// Adds the loose refs under `dir` (named `prefix/...`) to `raw`; a loose ref replaces the
/// packed one of the same name, and its packed peeled value goes with it.
// Used by `local_refs`.
fn collect_loose_refs(dir: &Path, prefix: &str, raw: &mut BTreeMap<String, String>, packed_peeled: &mut BTreeMap<String, String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = format!("{}/{}", prefix, entry.file_name().to_string_lossy());
        let path = entry.path();
        if path.is_dir() {
            collect_loose_refs(&path, &name, raw, packed_peeled);
        } else if let Ok(content) = fs::read_to_string(&path) {
            let content = content.trim();
            if content.starts_with("ref: ") || (content.len() == 40 && content.bytes().all(|b| b.is_ascii_hexdigit())) {
                packed_peeled.remove(&name);
                raw.insert(name, content.to_string());
            }
        }
    }
}

/// Follows a ref value (an id, or `ref: <target>` up to five levels deep) to an object id.
// Used by `local_refs`.
fn resolve_value(value: &str, raw: &BTreeMap<String, String>) -> Option<String> {
    let mut current = value.to_string();
    for _ in 0..5 {
        match current.strip_prefix("ref: ") {
            Some(target) => current = raw.get(target.trim())?.clone(),
            None => return Some(current),
        }
    }
    None
}

/// For an annotated tag, the object it finally points at (tags of tags are followed);
/// `None` for anything else.
// Used by `local_refs`.
fn peel(objects_dir: &Path, oid: &str) -> Option<String> {
    let mut current = oid.to_string();
    let mut peeled = None;
    for _ in 0..10 {
        let (kind, content) = crate::core::pack::read_object_in_objects_dir(objects_dir, &current)?;
        if kind != "tag" {
            return peeled;
        }
        let text = String::from_utf8_lossy(&content);
        current = text.lines().find_map(|l| l.strip_prefix("object "))?.trim().to_string();
        peeled = Some(current.clone());
    }
    peeled
}

/// The refs in a bundle's header, last first: git's bundle transport builds its ref list by
/// prepending, so `git ls-remote <bundle>` prints them in reverse header order.
// Used by `ls-remote` over the bundle transport.
pub fn bundle_refs(path: &Path) -> Result<Vec<RemoteRef>> {
    let (header, _) = crate::remote::bundle::engine::read_bundle(&path.to_string_lossy())?;
    Ok(header.refs.into_iter().rev().map(|(oid, name)| RemoteRef { name, oid: Some(oid), symref_target: None, peeled: None }).collect())
}

/// What `ls-remote` prints for `refs`: each ref with an id (peeled tags as their own `^{}`
/// entries right after the tag), kept when it passes the `--heads`/`--tags`/`--refs` filters
/// and matches a pattern (git's `*/<pattern>` against `/<name>`).
// Used by `ls-remote`.
pub fn select_refs(refs: &[RemoteRef], options: &LsRemoteOptions) -> Vec<ListedRef> {
    let mut listed = Vec::new();
    for r in refs {
        let Some(oid) = &r.oid else { continue };
        listed.push(ListedRef { name: r.name.clone(), oid: oid.clone(), symref_target: r.symref_target.clone() });
        if let Some(peeled) = &r.peeled {
            listed.push(ListedRef { name: format!("{}^{{}}", r.name), oid: peeled.clone(), symref_target: None });
        }
    }
    let patterns: Vec<String> = options.patterns.iter().map(|p| format!("*/{}", p)).collect();
    listed
        .into_iter()
        .filter(|r| passes_type_filter(&r.name, options))
        .filter(|r| patterns.is_empty() || patterns.iter().any(|p| crate::core::wildmatch::wildmatch(p, &format!("/{}", r.name), 0)))
        .collect()
}

/// git's `check_ref`: with no flags anything goes; otherwise only `refs/...` names, `--refs`
/// drops names that aren't valid ref names (the `^{}` entries), and `--heads` / `--tags` keep
/// their namespace.
// Used by `select_refs`.
fn passes_type_filter(name: &str, options: &LsRemoteOptions) -> bool {
    if !options.heads && !options.tags && !options.refs_only {
        return true;
    }
    let Some(rest) = name.strip_prefix("refs/") else { return false };
    if options.refs_only && rest.contains("^{}") {
        return false;
    }
    if options.heads && rest.starts_with("heads/") {
        return true;
    }
    if options.tags && rest.starts_with("tags/") {
        return true;
    }
    !options.heads && !options.tags
}

/// The listing as `ls-remote` prints it: `<id>\t<name>` lines, each symbolic ref preceded
/// by `ref: <target>\t<name>` with `--symref`.
// Used by `ls-remote`.
pub fn format_listing(refs: &[ListedRef], symref: bool) -> String {
    let mut out = String::new();
    for r in refs {
        if symref && let Some(target) = &r.symref_target {
            out.push_str(&format!("ref: {}\t{}\n", target, r.name));
        }
        out.push_str(&format!("{}\t{}\n", r.oid, r.name));
    }
    out
}

#[cfg(test)]
#[path = "../../../tests/unit/remote/ls_remote/engine.rs"]
mod tests;
