//! Rename and copy detection: a port of git's `diffcore-rename.c` (with the similarity
//! estimate of `diffcore-delta.c`), run on a list of changed paths before they are shown.
//! Exact renames come first, then (renames only) files with the same unique base name at 75%,
//! then the similarity table: renames before copies, the best four candidates per destination.

pub mod types;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::core::config::get_config_value;
use crate::core::object_store::{hash_content, read_object};
use crate::diff::types::{DiffChange, DiffSide};
use crate::diff::rename::types::RenameOptions;

static RENAME_LIMIT_NEEDED: AtomicUsize = AtomicUsize::new(0);
static RENAME_DEGRADED: AtomicBool = AtomicBool::new(false);

/// git's `diff_warn_rename_limit`: after a command, says when the rename limit cut detection
/// short, and which `<varname>` value would have been enough.
// Used by `diff`, `log`/`show`/`whatchanged` and `diff-tree`/`diff-index`/`diff-files`.
pub fn warn_rename_limit(varname: &str) {
    use std::io::Write;
    let needed = RENAME_LIMIT_NEEDED.load(Ordering::Relaxed);
    let degraded = RENAME_DEGRADED.load(Ordering::Relaxed);
    let _ = std::io::stdout().flush();
    if degraded {
        eprintln!("warning: only found copies from modified paths due to too many files.");
    } else if needed > 0 {
        eprintln!("warning: exhaustive rename detection was skipped due to too many files.");
    } else {
        return;
    }
    if needed > 0 {
        eprintln!("warning: you may want to set your {} variable to at least {} and retry the command.", varname, needed);
    }
}

/// git's MAX_SCORE: similarity is counted out of 60000.
pub const MAX_SCORE: u64 = 60000;
const HASHBASE: u32 = 107927;
const CANDIDATES: usize = 4;

/// git's `hash_chars`: the content cut into lines (at most 64 bytes each), each chunk hashed,
/// with the number of bytes per hash. CR before LF is skipped in text, and an unterminated
/// last line is dropped (git never adds it).
// Used by `estimate_similarity`, rename detection and `log --follow`.
pub fn span_hash(data: &[u8]) -> Vec<(u32, u64)> {
    let is_text = !data[..data.len().min(8000)].contains(&0);
    let mut counts: HashMap<u32, u64> = HashMap::new();
    let (mut accum1, mut accum2, mut n) = (0u32, 0u32, 0u64);
    let mut i = 0;
    while i < data.len() {
        let c = data[i] as u32;
        i += 1;
        if is_text && c == u32::from(b'\r') && i < data.len() && data[i] == b'\n' {
            continue;
        }
        let old_1 = accum1;
        accum1 = (accum1 << 7) ^ (accum2 >> 25);
        accum2 = (accum2 << 7) ^ (old_1 >> 25);
        accum1 = accum1.wrapping_add(c);
        n += 1;
        if n < 64 && c != u32::from(b'\n') {
            continue;
        }
        let hashval = accum1.wrapping_add(accum2.wrapping_mul(0x61)) % HASHBASE;
        *counts.entry(hashval).or_default() += n;
        n = 0;
        accum1 = 0;
        accum2 = 0;
    }
    let mut list: Vec<(u32, u64)> = counts.into_iter().collect();
    list.sort();
    list
}

/// git's `estimate_similarity`: how much of the larger file is material copied from the source,
/// out of 60000; 0 when the sizes differ too much to reach `min_score`.
// Used by rename detection and `log --follow`.
pub fn estimate_similarity(src: &[u8], src_hash: &[(u32, u64)], dst: &[u8], dst_hash: &[(u32, u64)], min_score: u64) -> u64 {
    let (src_size, dst_size) = (src.len() as u64, dst.len() as u64);
    let max_size = src_size.max(dst_size);
    let delta_size = max_size - src_size.min(dst_size);
    if max_size * (MAX_SCORE - min_score) < delta_size * MAX_SCORE || dst_size == 0 {
        return 0;
    }
    // diffcore_count_changes: bytes of dst whose chunk hash also occurs in src.
    let mut copied = 0u64;
    let mut d = 0;
    for &(hash, src_cnt) in src_hash {
        while d < dst_hash.len() && dst_hash[d].0 < hash {
            d += 1;
        }
        if d < dst_hash.len() && dst_hash[d].0 == hash {
            copied += src_cnt.min(dst_hash[d].1);
            d += 1;
        }
    }
    copied * MAX_SCORE / max_size
}

/// git's `pprint_rename`: `old => new`, with a common leading and trailing directory part
/// pulled out into `prefix{old => new}suffix`.
// Used by `--stat`, `--numstat` and `--summary` for renames, and `log --follow`.
pub fn pprint_rename(a: &str, b: &str) -> String {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let mut pfx = 0;
    let mut k = 0;
    while k < ab.len() && k < bb.len() && ab[k] == bb[k] {
        if ab[k] == b'/' {
            pfx = k + 1;
        }
        k += 1;
    }
    // Walk back from the terminating NUL; with a prefix, one byte into it (its slash).
    let mut sfx = 0;
    let adjust = usize::from(pfx > 0) as isize;
    let (mut oa, mut ob) = (ab.len() as isize, bb.len() as isize);
    let at = |s: &[u8], i: isize| if i as usize == s.len() { 0u8 } else { s[i as usize] };
    while pfx as isize - adjust <= oa && pfx as isize - adjust <= ob && oa >= 0 && ob >= 0 && at(ab, oa) == at(bb, ob) {
        if at(ab, oa) == b'/' {
            sfx = ab.len() - oa as usize;
        }
        oa -= 1;
        ob -= 1;
    }
    let a_mid = ab.len().saturating_sub(pfx + sfx);
    let b_mid = bb.len().saturating_sub(pfx + sfx);
    let mut out = String::new();
    if pfx + sfx > 0 {
        out.push_str(&a[..pfx]);
        out.push('{');
    }
    out.push_str(&a[pfx..pfx + a_mid]);
    out.push_str(" => ");
    out.push_str(&b[pfx..pfx + b_mid]);
    if pfx + sfx > 0 {
        out.push('}');
        out.push_str(&a[a.len() - sfx..]);
    }
    out
}

/// git's `parse_rename_score`: `50`, `5` and `.5` all mean 50%, `50%` too; returns the score
/// out of 60000 and the rest of the text.
fn parse_rename_score(text: &str) -> (u32, &str) {
    let (mut num, mut scale, mut dot) = (0u64, 1u64, false);
    let mut used = 0;
    for (i, ch) in text.char_indices() {
        if !dot && ch == '.' {
            scale = 1;
            dot = true;
        } else if ch == '%' {
            scale = if dot { scale * 100 } else { 100 };
            used = i + 1;
            break;
        } else if ch.is_ascii_digit() {
            if scale < 100000 {
                scale *= 10;
                num = num * 10 + u64::from(ch as u8 - b'0');
            }
        } else {
            used = i;
            break;
        }
        used = i + 1;
    }
    let score = if num >= scale { MAX_SCORE } else { MAX_SCORE * num / scale };
    (score as u32, &text[used..])
}

/// The rename setting the porcelain commands start from: `diff.renames` (default on;
/// `copies`/`copy` also finds copies; false turns detection off), and `diff.renameLimit`.
// Used by `diff`, `log`/`show`/`whatchanged` and `status`.
pub fn default_rename_options(config_key: &str) -> RenameOptions {
    let mut opts = RenameOptions { level: 1, ..Default::default() };
    let value = get_config_value(config_key).ok().flatten().or_else(|| if config_key == "diff.renames" { None } else { get_config_value("diff.renames").ok().flatten() });
    if let Some(value) = value {
        opts.level = match value.to_ascii_lowercase().as_str() {
            "copies" | "copy" => 2,
            "false" | "no" | "off" | "0" => 0,
            _ => 1,
        };
    }
    if let Some(limit) = get_config_value("diff.renameLimit").ok().flatten().and_then(|v| v.parse().ok()) {
        opts.limit = limit;
    }
    opts
}

/// Applies one rename option to `opts` if `arg` is one: `-M[<n>]`, `--find-renames[=<n>]`,
/// `-C[<n>]`, `--find-copies[=<n>]`, `--find-copies-harder`, `-l<n>`, `--no-renames`.
/// `-C` given twice means `--find-copies-harder`, as in git.
// Used by `diff`, `log`, `diff-tree`/`diff-index`/`diff-files` and `fast-export`.
pub fn parse_rename_option(arg: &str, opts: &mut RenameOptions) -> bool {
    let score = |rest: &str, opts: &mut RenameOptions| {
        if !rest.is_empty() {
            opts.min_score = parse_rename_score(rest).0;
        }
    };
    if let Some(rest) = arg.strip_prefix("--find-renames") {
        opts.level = opts.level.max(1);
        score(rest.trim_start_matches('='), opts);
    } else if let Some(rest) = arg.strip_prefix("--find-copies-harder") {
        let _ = rest;
        opts.level = 2;
        opts.find_copies_harder = true;
    } else if let Some(rest) = arg.strip_prefix("--find-copies") {
        if opts.level == 2 {
            opts.find_copies_harder = true;
        }
        opts.level = 2;
        score(rest.trim_start_matches('='), opts);
    } else if arg == "--no-renames" {
        opts.level = 0;
    } else if let Some(rest) = arg.strip_prefix("-M") {
        opts.level = opts.level.max(1);
        score(rest, opts);
    } else if let Some(rest) = arg.strip_prefix("-C") {
        if opts.level == 2 {
            opts.find_copies_harder = true;
        }
        opts.level = 2;
        score(rest, opts);
    } else if let Some(n) = arg.strip_prefix("-l").and_then(|n| n.parse().ok()) {
        opts.limit = n;
    } else {
        return false;
    }
    true
}

fn is_regular(mode: u32) -> bool {
    mode & 0o170000 == 0o100000
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The content of one side of a change (worktree files are read from disk).
fn side_bytes(path: &str, side: &DiffSide) -> Vec<u8> {
    if side.worktree {
        return std::fs::read(path).unwrap_or_default();
    }
    read_object(&hex::encode(side.hash)).map(|o| o.1).unwrap_or_default()
}

/// The blob id of a side (worktree content hashed on the fly).
fn side_id(path: &str, side: &DiffSide) -> [u8; 20] {
    if side.worktree {
        return hash_content("blob", &std::fs::read(path).unwrap_or_default());
    }
    side.hash
}

struct Source {
    path: String,
    side: DiffSide,
    /// git's `rename_used`: starts at 1 for a file that stays (a copy source).
    used: u32,
    deleted: bool,
    id: [u8; 20],
}

/// git's diffcore_rename: finds renames (and copies with `level` 2) among `changes` (in path
/// order, already limited to the pathspecs) and returns the new list. A detected pair takes the
/// place of its destination; a deleted file that was renamed disappears. `unmodified` lists the
/// files that did not change, the extra copy sources of `--find-copies-harder`.
// Used by `diff`, `log`/`show`, `status`, `diff-tree`/`diff-index`/`diff-files` and `fast-export`.
pub fn detect_renames(changes: Vec<DiffChange>, opts: &RenameOptions, unmodified: &[(String, DiffSide)]) -> Vec<DiffChange> {
    if opts.level == 0 {
        return changes;
    }
    let copies = opts.level >= 2;
    let usable = |side: &DiffSide| side.mode & 0o170000 != 0o160000 && side.mode & 0o170000 != 0o040000;

    // Destinations: creations. Sources: deletions, plus (copies) modified files, plus
    // (--find-copies-harder) unchanged files, all in path order.
    let dsts: Vec<usize> = (0..changes.len()).filter(|&i| !changes[i].unmerged && changes[i].old.is_none() && changes[i].new.as_ref().is_some_and(usable)).collect();
    if dsts.is_empty() {
        return changes;
    }
    let mut sources: Vec<Source> = Vec::new();
    // Unmerged paths take no part in rename detection.
    for change in changes.iter().filter(|c| !c.unmerged) {
        match (&change.old, &change.new) {
            (Some(old), None) if usable(old) => sources.push(Source { path: change.path.clone(), side: old.clone(), used: 0, deleted: true, id: old.hash }),
            (Some(old), Some(_)) if copies && usable(old) => sources.push(Source { path: change.path.clone(), side: old.clone(), used: 1, deleted: false, id: old.hash }),
            _ => {}
        }
    }
    if copies && opts.find_copies_harder {
        for (path, side) in unmodified {
            if usable(side) {
                sources.push(Source { path: path.clone(), side: side.clone(), used: 1, deleted: false, id: side.hash });
            }
        }
    }
    sources.sort_by(|a, b| a.path.cmp(&b.path));
    if sources.is_empty() {
        return changes;
    }

    // dst index (into `changes`) -> (source index, score)
    let mut found: HashMap<usize, (usize, u64)> = HashMap::new();
    let dst_side = |d: usize| changes[d].new.clone().expect("creation");
    let dst_ids: Vec<[u8; 20]> = dsts.iter().map(|&d| side_id(&changes[d].path, &dst_side(d))).collect();

    // 1. Exact renames: same blob; an unused source first, then the same base name.
    for (n, &d) in dsts.iter().enumerate() {
        let target = dst_side(d);
        let mut best: Option<(u32, usize)> = None;
        for (j, src) in sources.iter().enumerate() {
            if src.id != dst_ids[n] || is_regular(src.side.mode) != is_regular(target.mode) {
                continue;
            }
            if src.used > 0 && !copies {
                continue;
            }
            let score = u32::from(src.used == 0) + u32::from(basename(&src.path) == basename(&changes[d].path));
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, j));
            }
        }
        if let Some((_, j)) = best {
            sources[j].used += 1;
            found.insert(d, (j, MAX_SCORE));
        }
    }

    let mut contents: HashMap<usize, (Vec<u8>, Vec<(u32, u64)>)> = HashMap::new();
    let mut src_content = |j: usize, sources: &[Source]| -> (Vec<u8>, Vec<(u32, u64)>) {
        contents
            .entry(j)
            .or_insert_with(|| {
                let bytes = side_bytes(&sources[j].path, &sources[j].side);
                let hash = span_hash(&bytes);
                (bytes, hash)
            })
            .clone()
    };
    let min_score = u64::from(opts.min_score);

    // 2. Renames only: files with the same base name, unique on both sides, at 75% or more.
    if !copies {
        let min_basename_score = min_score + (MAX_SCORE - min_score) / 2;
        let mut src_by_base: HashMap<&str, Option<usize>> = HashMap::new();
        for (j, src) in sources.iter().enumerate().filter(|(_, s)| s.used == 0) {
            src_by_base.entry(basename(&src.path)).and_modify(|e| *e = None).or_insert(Some(j));
        }
        let mut dst_by_base: HashMap<&str, Option<usize>> = HashMap::new();
        for &d in dsts.iter().filter(|d| !found.contains_key(d)) {
            dst_by_base.entry(basename(&changes[d].path)).and_modify(|e| *e = None).or_insert(Some(d));
        }
        let mut pairs: Vec<(usize, usize)> = src_by_base
            .iter()
            .filter_map(|(base, j)| Some(((*j)?, (*dst_by_base.get(base)?)?)))
            .collect();
        pairs.sort();
        for (j, d) in pairs {
            let (src_bytes, src_hash) = src_content(j, &sources);
            let dst_bytes = side_bytes(&changes[d].path, &dst_side(d));
            let score = if is_regular(sources[j].side.mode) && is_regular(dst_side(d).mode) {
                estimate_similarity(&src_bytes, &src_hash, &dst_bytes, &span_hash(&dst_bytes), min_basename_score)
            } else {
                0
            };
            if score >= min_basename_score {
                sources[j].used += 1;
                found.insert(d, (j, score));
            }
        }
    }

    // 3. The similarity table: the best four sources per remaining destination.
    let remaining_dsts: Vec<usize> = dsts.iter().copied().filter(|d| !found.contains_key(d)).collect();
    let mut remaining_srcs: Vec<usize> = (0..sources.len()).filter(|&j| copies || sources[j].used == 0).collect();
    // git's too_many_rename_candidates: past `limit` x `limit`, skip the table (or, under
    // --find-copies-harder, drop the unchanged sources if that is enough), and warn later.
    let mut run_table = !remaining_dsts.is_empty() && !remaining_srcs.is_empty();
    let limit = opts.limit;
    if run_table && limit > 0 && remaining_dsts.len() * remaining_srcs.len() > limit * limit {
        RENAME_LIMIT_NEEDED.fetch_max(remaining_srcs.len().max(remaining_dsts.len()), Ordering::Relaxed);
        let modified: Vec<usize> = remaining_srcs.iter().copied().filter(|&j| sources[j].deleted || changes.iter().any(|c| c.path == sources[j].path)).collect();
        if opts.find_copies_harder && remaining_dsts.len() * modified.len() <= limit * limit {
            RENAME_DEGRADED.store(true, Ordering::Relaxed);
            remaining_srcs = modified;
        } else {
            run_table = false;
        }
    }
    if run_table {
        // (score, same base name, dst, src); `None` is an unused slot.
        type Slot = Option<(u64, bool, usize, usize)>;
        let compare = |a: &Slot, b: &Slot| -> std::cmp::Ordering {
            use std::cmp::Ordering::*;
            match (a, b) {
                (None, None) => Equal,
                (None, Some(_)) => Greater,
                (Some(_), None) => Less,
                (Some(a), Some(b)) if a.0 == b.0 => b.1.cmp(&a.1),
                (Some(a), Some(b)) => b.0.cmp(&a.0),
            }
        };
        let mut table: Vec<Slot> = Vec::new();
        for &d in &remaining_dsts {
            let dst = dst_side(d);
            let dst_bytes = side_bytes(&changes[d].path, &dst);
            let dst_hash = span_hash(&dst_bytes);
            let mut slots: [Slot; CANDIDATES] = [None; CANDIDATES];
            for &j in &remaining_srcs {
                let score = if is_regular(sources[j].side.mode) && is_regular(dst.mode) {
                    let (src_bytes, src_hash) = src_content(j, &sources);
                    estimate_similarity(&src_bytes, &src_hash, &dst_bytes, &dst_hash, min_score)
                } else {
                    0
                };
                let candidate = Some((score, basename(&sources[j].path) == basename(&changes[d].path), d, j));
                let mut worst = 0;
                for i in 1..CANDIDATES {
                    if compare(&slots[i], &slots[worst]) == std::cmp::Ordering::Greater {
                        worst = i;
                    }
                }
                if compare(&slots[worst], &candidate) == std::cmp::Ordering::Greater {
                    slots[worst] = candidate;
                }
            }
            table.extend(slots);
        }
        table.sort_by(compare);
        for copy_pass in [false, true] {
            if copy_pass && !copies {
                break;
            }
            for &(score, _, d, j) in table.iter().flatten() {
                if score < min_score {
                    break;
                }
                if found.contains_key(&d) || (!copy_pass && sources[j].used > 0) {
                    continue;
                }
                sources[j].used += 1;
                found.insert(d, (j, score));
            }
        }
    }

    // Build the output: a detected pair replaces its destination; a deleted source that was
    // used disappears. The last user of a deleted source is the rename, the others are copies.
    let used_deleted: std::collections::HashSet<String> = found.values().filter(|&&(j, _)| sources[j].deleted).map(|&(j, _)| sources[j].path.clone()).collect();
    let mut output = Vec::new();
    for (i, change) in changes.iter().enumerate() {
        if let Some(&(j, score)) = found.get(&i) {
            let src = &mut sources[j];
            src.used -= 1;
            let status = if src.used > 0 || !src.deleted { 'C' } else { 'R' };
            // A working-tree destination was hashed for the comparison; git shows that id.
            let new = change.new.clone().map(|mut side| {
                if side.worktree {
                    side.hash = side_id(&change.path, &side);
                }
                side
            });
            output.push(DiffChange {
                path: change.path.clone(),
                old: Some(src.side.clone()),
                new,
                renamed_from: Some((src.path.clone(), status, score as u32)),
                unmerged: false,
            });
            continue;
        }
        if !(change.new.is_none() && used_deleted.contains(&change.path)) {
            let mut change = change.clone();
            // Every creation was hashed for the exact-rename pass (git shows its id).
            if dsts.contains(&i) {
                if let Some(side) = change.new.as_mut().filter(|s| s.worktree) {
                    side.hash = side_id(&change.path, side);
                }
            }
            output.push(change);
        }
    }
    output
}

