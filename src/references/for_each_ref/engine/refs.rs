//! Ref reading and collection: loose and packed refs, symref chains, pattern filters, `--points-at`, worktree branches.

use crate::core::{
    check_ref_format::check_refname_format,
    commit::collect_ancestors,
    object_store::read_object,
    wildmatch::{wildmatch, WM_CASEFOLD, WM_PATHNAME},
};
use crate::references::{
    for_each_ref::engine::PER_WORKTREE_REFS,
    for_each_ref::engine::atoms::{peel_to_commit, tag_target},
    for_each_ref::types::{ForEachRefOptions, RefArrayItem},
};
use std::fs;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// The ref HEAD names, followed through symbolic refs, when it exists (git's
/// `resolve_refdup("HEAD")`): `refs/heads/<branch>`, `HEAD` when detached, `None` when unborn.
pub(super) fn resolve_head_ref() -> Option<String> {
    resolve_ref_chain("HEAD").map(|(name, _, _)| name)
}

/// Reads one ref without following symbolic refs: `Some(Ok(hash))`, `Some(Err(target))` for a
/// `ref: ` symref, along with whether it came from `packed-refs`. `None` if it doesn't exist or
/// is unreadable.
fn read_raw_ref(name: &str) -> Option<(Result<String, String>, bool)> {
    let path = crate::core::repo::path(format!(".git/{}", name));
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
    if let Ok(packed) = fs::read_to_string(crate::core::repo::path(".git/packed-refs")) {
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
pub(super) fn resolve_ref_chain(name: &str) -> Option<(String, String, bool)> {
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

pub(super) fn ref_exists(name: &str) -> bool {
    resolve_ref_chain(name).is_some()
}

/// Lists every ref under `refs/` (loose and packed, per-worktree refs from the current
/// worktree) in name order, like git's ref iteration (only those under `prefixes`, when
/// given): refs with bad names or unreadable
/// contents are skipped with git's warnings, dangling symbolic refs silently.
// Used by `for-each-ref`.
pub fn collect_ref_items(prefixes: Option<&[String]>) -> Vec<RefArrayItem> {
    let linked = crate::core::repo::is_linked_worktree();
    let mut loose: BTreeMap<String, String> = BTreeMap::new();
    walk_loose_refs(&crate::core::repo::path(".git/refs"), "refs", linked, &mut loose);
    if linked {
        for prefix in PER_WORKTREE_REFS {
            walk_loose_refs(&crate::core::repo::path(format!(".git/{}", prefix)), prefix, false, &mut loose);
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

pub(super) fn points_at(targets: &[String], oid: &str) -> bool {
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
    if let Ok(worktrees) = crate::workspace::worktree::engine::list_worktrees() {
        for wt in worktrees {
            if let Some(branch) = wt.branch {
                out.entry(branch).or_insert_with(|| crate::core::repo::to_git_path_string(&wt.path));
            }
        }
    }
    out
}
