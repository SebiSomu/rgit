//! Ref name shortening and remote tracking: `:lstrip`/`:rstrip`/`:short`, refspecs, upstream/push refs, ahead/behind.

use crate::core::{
    commit::collect_ancestors,
    config::{get_config_value, global_config_path, LOCAL_CONFIG_PATH, normalize_key, read_entries_from},
};
use crate::references::{
    for_each_ref::engine::SHORTEN_RULES,
    for_each_ref::engine::atoms::{parse_maybe_bool, peel_to_commit},
    for_each_ref::engine::refs::{ref_exists, resolve_ref_chain},
    for_each_ref::types::{RefAtomArg, RefnameShow, RemoteRefShow},
};

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
    shorten_unambiguous_ref_with(refname, strict)
}

/// Shortens a full ref name like `shorten_unambiguous_ref`, with the strictness given by the
/// caller: strict mode needs every other rule to miss, non-strict only the rules before the
/// matching one (what `symbolic-ref --short` uses).
// Used by `shorten_unambiguous_ref` and `symbolic-ref --short`.
pub fn shorten_unambiguous_ref_with(refname: &str, strict: bool) -> String {
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

pub(super) fn show_ref_name(show: RefnameShow, refname: &str) -> String {
    match show {
        RefnameShow::Full => refname.to_string(),
        RefnameShow::Short => shorten_unambiguous_ref(refname),
        RefnameShow::Lstrip(n) => lstrip_ref(refname, n),
        RefnameShow::Rstrip(n) => rstrip_ref(refname, n),
    }
}

pub(super) fn config_values(key: &str) -> Vec<String> {
    let Ok(key) = normalize_key(key) else { return Vec::new() };
    let mut out = Vec::new();
    let mut paths = Vec::new();
    if let Some(global) = global_config_path() {
        paths.push(global);
    }
    paths.push(crate::core::repo::path(LOCAL_CONFIG_PATH));
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

/// The full upstream ref of a local branch (`branch.<b>.remote` + `.merge` through the remote's
/// fetch refspecs; a `.` remote names a local ref), like git's `branch_get_upstream`.
// Used by `for-each-ref` (`%(upstream)`) and `branch -v` / `-vv`.
pub fn branch_upstream(branch: &str) -> Option<String> {
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

pub(super) fn branch_push_ref(branch: &str) -> Option<String> {
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

/// (ahead, behind) commit counts of `local` against `base` (both full refs); None when
/// either is missing (git's "gone" upstream).
// Used by `for-each-ref` (`%(upstream:track)`) and `branch -v` / `-vv`.
pub fn tracking_counts(local: &str, base: &str) -> Option<(usize, usize)> {
    let theirs = peel_to_commit(&resolve_ref_chain(base)?.1)?;
    let ours = peel_to_commit(&resolve_ref_chain(local)?.1)?;
    if theirs == ours {
        return Some((0, 0));
    }
    Some(ahead_behind(&ours, &theirs))
}

pub(super) fn ahead_behind(tip: &str, base: &str) -> (usize, usize) {
    let tip_set = collect_ancestors(tip).unwrap_or_default();
    let base_set = collect_ancestors(base).unwrap_or_default();
    (tip_set.difference(&base_set).count(), base_set.difference(&tip_set).count())
}

pub(super) fn remote_ref_details(arg: &RefAtomArg, refname: Option<&str>, branch: &str) -> String {
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
