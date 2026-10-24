use std::collections::{BTreeMap, BTreeSet};
use crate::helpers::format_patch::compacted_changes;
use crate::helpers::{find_all_merge_bases, read_object, resolve_tree_from_source, write_object};
use crate::objects::{ConflictRegion, FileMergeResult, MergeMessage, MergePathOutcome, MergePathReport, SideChange, TreeMergeResult};

type Entry = ([u8; 20], u32);

/// A run of base lines `[start, end)` that one side replaced with `lines`.
struct Change {
    start: usize,
    end: usize,
    lines: Vec<String>,
}

fn side_changes(base: &[&str], side: &[&str]) -> Vec<Change> {
    let (changes, _, _) = compacted_changes(&to_strings(base), &to_strings(side), false);
    changes.into_iter().map(|(i, n, j, m)| Change { start: i, end: i + n, lines: to_strings(&side[j..j + m]) }).collect()
}

/// Applies one side's changes that fall inside base lines `[start, end)`.
fn side_version(base: &[&str], changes: &[Change], start: usize, end: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = start;
    for change in changes {
        out.extend(base[i..change.start].iter().map(|s| s.to_string()));
        out.extend(change.lines.iter().cloned());
        i = change.end;
    }
    out.extend(base[i..end].iter().map(|s| s.to_string()));
    out
}

/// What a run of merged lines is: unchanged on both sides, a change from one side or from both
/// (identical), or a conflict. `ours` / `theirs` hold the run as it reads in each side's file,
/// and `base` its range of base lines.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ChunkKind {
    Stable,
    Ours,
    Theirs,
    Both,
    Conflict,
}

struct Chunk {
    kind: ChunkKind,
    ours: Vec<String>,
    theirs: Vec<String>,
    base: (usize, usize),
}

fn to_strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|s| s.to_string()).collect()
}

/// git's `is_eol_crlf`: 1 if the line ends in CRLF, 0 for LF, -1 when it can't be told.
fn eol_crlf(lines: &[&str], i: usize) -> i32 {
    let ends_crlf = |l: &str| l.ends_with("\r\n") as i32;
    if lines.is_empty() {
        return -1;
    }
    if i + 1 < lines.len() || lines[i].ends_with('\n') {
        return ends_crlf(lines[i]);
    }
    if i == 0 { -1 } else { ends_crlf(lines[i - 1]) }
}

/// Merges three versions of a text file line by line, the way git's xdiff merge does at its
/// default (`zealous`) level: each side's changes against the base are found with xdiff's
/// compacted Myers diff; changes that don't overlap or touch are both applied; overlapping or adjacent ones
/// form a conflict unless both sides made the identical change; each conflict is refined by
/// diffing ours against theirs so lines both sides agree on stay outside the markers; and
/// conflicts separated by at most 3 unchanged lines are joined back into one. Conflicts are
/// written as `<<<<<<< ours_label` / `=======` / `>>>>>>> theirs_label` with `marker_size`
/// marker characters, using CRLF when the surrounding lines do.
// Used by `merge-tree` via `merge_trees_in_memory`.
pub fn merge_file_lines(base: &str, ours: &str, theirs: &str, ours_label: &str, theirs_label: &str, marker_size: usize) -> FileMergeResult {
    let base_lines: Vec<&str> = base.split_inclusive('\n').collect();
    let our_lines: Vec<&str> = ours.split_inclusive('\n').collect();
    let their_lines: Vec<&str> = theirs.split_inclusive('\n').collect();
    let ours_changes = side_changes(&base_lines, &our_lines);
    let theirs_changes = side_changes(&base_lines, &their_lines);

    let mut result = FileMergeResult::default();
    let mut chunks: Vec<Chunk> = Vec::new();
    let stable = |from: usize, to: usize| Chunk { kind: ChunkKind::Stable, ours: to_strings(&base_lines[from..to]), theirs: to_strings(&base_lines[from..to]), base: (from, to) };
    let (mut p, mut q, mut pos) = (0, 0, 0);

    while p < ours_changes.len() || q < theirs_changes.len() {
        let a = ours_changes.get(p);
        let b = theirs_changes.get(q);
        let ours_first = matches!((a, b), (Some(a), Some(b)) if a.end < b.start) || b.is_none();
        let theirs_first = matches!((a, b), (Some(a), Some(b)) if b.end < a.start) || a.is_none();
        if ours_first || theirs_first {
            let change = if ours_first { a.unwrap() } else { b.unwrap() };
            chunks.push(stable(pos, change.start));
            let unchanged = to_strings(&base_lines[change.start..change.end]);
            let (kind, ours, theirs) = if ours_first {
                result.from_ours += 1;
                p += 1;
                (ChunkKind::Ours, change.lines.clone(), unchanged)
            } else {
                result.from_theirs += 1;
                q += 1;
                (ChunkKind::Theirs, unchanged, change.lines.clone())
            };
            chunks.push(Chunk { kind, ours, theirs, base: (change.start, change.end) });
            pos = change.end;
            continue;
        }

        let (a, b) = (a.unwrap(), b.unwrap());
        let (start, mut end) = (a.start.min(b.start), a.end.max(b.end));
        let (p0, q0) = (p, q);
        p += 1;
        q += 1;
        loop {
            if let Some(c) = ours_changes.get(p).filter(|c| c.start <= end) {
                end = end.max(c.end);
                p += 1;
            } else if let Some(c) = theirs_changes.get(q).filter(|c| c.start <= end) {
                end = end.max(c.end);
                q += 1;
            } else {
                break;
            }
        }

        chunks.push(stable(pos, start));
        pos = end;
        let ours_part = side_version(&base_lines, &ours_changes[p0..p], start, end);
        let theirs_part = side_version(&base_lines, &theirs_changes[q0..q], start, end);
        if ours_part == theirs_part {
            result.from_both += 1;
            chunks.push(Chunk { kind: ChunkKind::Both, ours: ours_part.clone(), theirs: ours_part, base: (start, end) });
            continue;
        }
        if ours_part.is_empty() || theirs_part.is_empty() {
            chunks.push(Chunk { kind: ChunkKind::Conflict, ours: ours_part, theirs: theirs_part, base: (start, end) });
            continue;
        }

        let (refined, _, _) = compacted_changes(&ours_part, &theirs_part, false);
        let mut i = 0;
        for (oi, on, tj, tn) in refined {
            let common = ours_part[i..oi].to_vec();
            if !common.is_empty() {
                chunks.push(Chunk { kind: ChunkKind::Stable, ours: common.clone(), theirs: common, base: (start, start) });
            }
            chunks.push(Chunk { kind: ChunkKind::Conflict, ours: ours_part[oi..oi + on].to_vec(), theirs: theirs_part[tj..tj + tn].to_vec(), base: (start, end) });
            i = oi + on;
        }
        let common = ours_part[i..].to_vec();
        if !common.is_empty() {
            chunks.push(Chunk { kind: ChunkKind::Stable, ours: common.clone(), theirs: common, base: (start, start) });
        }
    }
    chunks.push(stable(pos, base_lines.len()));
    chunks.retain(|c| c.kind != ChunkKind::Stable || !c.ours.is_empty());

    // git's `xdl_simplify_non_conflicts`: up to 3 unchanged lines between two conflicts join them into one.
    let mut joined: Vec<Chunk> = Vec::new();
    for chunk in chunks {
        let n = joined.len();
        let after_conflict = |i: usize| joined[i].kind == ChunkKind::Conflict;
        let joinable = chunk.kind == ChunkKind::Conflict
            && n >= 1
            && (after_conflict(n - 1) || (n >= 2 && joined[n - 1].kind == ChunkKind::Stable && joined[n - 1].ours.len() <= 3 && after_conflict(n - 2)));
        if !joinable {
            joined.push(chunk);
            continue;
        }
        let gap = if joined[n - 1].kind == ChunkKind::Stable { joined.pop() } else { None };
        let last = joined.last_mut().unwrap();
        if let Some(gap) = gap {
            last.ours.extend(gap.ours);
            last.theirs.extend(gap.theirs);
        }
        last.ours.extend(chunk.ours);
        last.theirs.extend(chunk.theirs);
        last.base = (last.base.0.min(chunk.base.0), last.base.1.max(chunk.base.1));
    }

    let mut out = String::new();
    let (mut ours_pos, mut theirs_pos) = (0usize, 0usize);
    for chunk in joined {
        match chunk.kind {
            ChunkKind::Conflict => {
                let mut crlf = eol_crlf(&our_lines, ours_pos.saturating_sub(1));
                if crlf != 0 {
                    crlf = eol_crlf(&their_lines, theirs_pos.saturating_sub(1));
                }
                if crlf != 0 {
                    crlf = eol_crlf(&base_lines, 0);
                }
                let eol = if crlf > 0 { "\r\n" } else { "\n" };
                let push_side = |out: &mut String, lines: &[String]| {
                    lines.iter().for_each(|l| out.push_str(l));
                    if lines.last().is_some_and(|l| !l.ends_with('\n')) {
                        out.push_str(eol);
                    }
                };
                out.push_str(&format!("{} {}{}", "<".repeat(marker_size), ours_label, eol));
                push_side(&mut out, &chunk.ours);
                out.push_str(&format!("{}{}", "=".repeat(marker_size), eol));
                push_side(&mut out, &chunk.theirs);
                out.push_str(&format!("{} {}{}", ">".repeat(marker_size), theirs_label, eol));
                result.conflicts.push(ConflictRegion {
                    base_line: chunk.base.0 + 1,
                    base: to_strings(&base_lines[chunk.base.0..chunk.base.1]),
                    ours: chunk.ours.clone(),
                    theirs: chunk.theirs.clone(),
                });
            }
            ChunkKind::Theirs => chunk.theirs.iter().for_each(|l| out.push_str(l)),
            _ => chunk.ours.iter().for_each(|l| out.push_str(l)),
        }
        ours_pos += chunk.ours.len();
        theirs_pos += chunk.theirs.len();
    }
    result.content = out.into_bytes();
    result
}

fn read_blob(entry: Option<&Entry>) -> anyhow::Result<Vec<u8>> {
    match entry {
        Some((hash, _)) => Ok(read_object(&hex::encode(hash))?.1),
        None => Ok(Vec::new()),
    }
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8000)].contains(&0)
}

fn message(paths: &[&str], kind: &str, text: String) -> MergeMessage {
    MergeMessage { paths: paths.iter().map(|p| p.to_string()).collect(), kind: kind.to_string(), text }
}

/// Merges three flattened trees path by path, like git's `ort` strategy without rename
/// detection: a path one side left alone takes the other side's version; a file both sides
/// edited is merged line by line (conflict markers written into the merged blob); and
/// modify/delete, add/add, binary, mode, and file/directory clashes become conflicts,
/// reported with git's messages and recorded with their index stages. Merged blobs are written
/// to the object store. `marker_size` grows for the inner merges that build a virtual base.
// Used by `merge-tree`.
pub fn merge_trees_in_memory(base: &BTreeMap<String, Entry>, ours: &BTreeMap<String, Entry>, theirs: &BTreeMap<String, Entry>, ours_label: &str, theirs_label: &str, marker_size: usize) -> anyhow::Result<TreeMergeResult> {
    let mut result = TreeMergeResult::default();
    let paths: BTreeSet<&String> = base.keys().chain(ours.keys()).chain(theirs.keys()).collect();

    for path in paths {
        let (b, o, t) = (base.get(path), ours.get(path), theirs.get(path));
        let (our_change, their_change) = (SideChange::between(b, o), SideChange::between(b, t));
        let report = |outcome: MergePathOutcome, file: Option<FileMergeResult>, result: &mut TreeMergeResult| {
            if our_change != SideChange::Unchanged || their_change != SideChange::Unchanged {
                result.reports.push(MergePathReport { path: path.clone(), ours: our_change, theirs: their_change, outcome, file });
            }
        };

        if o == t || t == b || o == b {
            let (entry, outcome) = if o == t {
                (o, MergePathOutcome::SameOnBothSides)
            } else if t == b {
                (o, MergePathOutcome::TakenFromOurs)
            } else {
                (t, MergePathOutcome::TakenFromTheirs)
            };
            if let Some(entry) = entry {
                result.tree.insert(path.clone(), *entry);
            }
            report(outcome, None, &mut result);
            continue;
        }

        let (Some(o), Some(t)) = (o, t) else {
            let (deleted_in, modified_in, kept) = if o.is_none() { (ours_label, theirs_label, t) } else { (theirs_label, ours_label, o) };
            result.tree.insert(path.clone(), *kept.unwrap());
            result.conflicted.insert(path.clone(), [b.copied(), o.copied(), t.copied()]);
            result.messages.push(message(
                &[path],
                "CONFLICT (modify/delete)",
                format!("CONFLICT (modify/delete): {} deleted in {} and modified in {}.  Version {} of {} left in tree.", path, deleted_in, modified_in, modified_in, path),
            ));
            report(MergePathOutcome::Conflict("modify/delete".into()), None, &mut result);
            continue;
        };

        let kind = if b.is_none() { "add/add" } else { "content" };
        let regular = |mode: u32| mode & 0o170000 == 0o100000;
        let (base_bytes, our_bytes, their_bytes) = (read_blob(b)?, read_blob(Some(o))?, read_blob(Some(t))?);
        let base_mode = b.map(|e| e.1);
        let (mode, mode_conflict) = if o.1 == t.1 || base_mode == Some(t.1) {
            (o.1, false)
        } else if base_mode == Some(o.1) {
            (t.1, false)
        } else {
            (o.1, true)
        };

        if !regular(o.1) || !regular(t.1) || is_binary(&base_bytes) || is_binary(&our_bytes) || is_binary(&their_bytes) {
            if regular(o.1) && regular(t.1) {
                result.messages.push(message(&[path], "CONFLICT (binary)", format!("warning: Cannot merge binary files: {} ({} vs. {})", path, ours_label, theirs_label)));
            }
            result.messages.push(message(&[path], "Auto-merging", format!("Auto-merging {}", path)));
            result.messages.push(message(&[path], "CONFLICT (contents)", format!("CONFLICT ({}): Merge conflict in {}", kind, path)));
            result.tree.insert(path.clone(), *o);
            result.conflicted.insert(path.clone(), [b.copied(), Some(*o), Some(*t)]);
            report(MergePathOutcome::Conflict("binary".into()), None, &mut result);
            continue;
        }

        let merged = merge_file_lines(
            &String::from_utf8_lossy(&base_bytes),
            &String::from_utf8_lossy(&our_bytes),
            &String::from_utf8_lossy(&their_bytes),
            ours_label,
            theirs_label,
            marker_size,
        );
        let blob = write_object("blob", &merged.content)?;
        let mut hash = [0u8; 20];
        hex::decode_to_slice(&blob, &mut hash)?;
        result.tree.insert(path.clone(), (hash, mode));
        result.messages.push(message(&[path], "Auto-merging", format!("Auto-merging {}", path)));

        let outcome = if !merged.conflicts.is_empty() {
            result.messages.push(message(&[path], "CONFLICT (contents)", format!("CONFLICT ({}): Merge conflict in {}", kind, path)));
            MergePathOutcome::Conflict(kind.into())
        } else if mode_conflict {
            result.messages.push(message(&[path], "CONFLICT (mode)", format!("CONFLICT (mode): {} had different modes in {} and {}; kept {}", path, ours_label, theirs_label, ours_label)));
            MergePathOutcome::Conflict("mode".into())
        } else {
            MergePathOutcome::AutoMerged
        };
        if matches!(outcome, MergePathOutcome::Conflict(_)) {
            result.conflicted.insert(path.clone(), [b.copied(), Some(*o), Some(*t)]);
        }
        report(outcome, Some(merged), &mut result);
    }

    resolve_file_directory_clashes(&mut result, ours, ours_label, theirs_label);
    Ok(result)
}

/// A merged file whose path is also a directory in the merged tree is moved to
/// `<path>~<side>` (with `/` in the side's name turned into `_`), as git does.
fn resolve_file_directory_clashes(result: &mut TreeMergeResult, ours: &BTreeMap<String, Entry>, ours_label: &str, theirs_label: &str) {
    let clashing: Vec<String> = result
        .tree
        .keys()
        .filter(|path| result.tree.range(format!("{}/", path)..).next().is_some_and(|(next, _)| next.starts_with(&format!("{}/", path))))
        .cloned()
        .collect();
    for path in clashing {
        let entry = result.tree.remove(&path).unwrap();
        let side = if ours.get(&path) == Some(&entry) { ours_label } else { theirs_label };
        let moved = format!("{}~{}", path, side.replace('/', "_"));
        result.tree.insert(moved.clone(), entry);
        let stage = if side == ours_label { [None, Some(entry), None] } else { [None, None, Some(entry)] };
        result.conflicted.insert(moved.clone(), stage);
        result.messages.push(message(
            &[&moved, &path],
            "CONFLICT (file/directory)",
            format!("CONFLICT (file/directory): directory in the way of {} from {}; moving it to {} instead.", path, side, moved),
        ));
        if let Some(report) = result.reports.iter_mut().find(|r| r.path == path) {
            report.outcome = MergePathOutcome::Conflict("file/directory".into());
        }
    }
}

/// Builds the base for merging two commits: the tree of their single merge base, or, for a
/// criss-cross history with several merge bases, a virtual base made by merging those bases
/// into each other first (recursively, with longer conflict markers), like git's `ort`.
/// Returns the tree and the merge bases used (empty for unrelated histories, which need
/// `allow_unrelated` and merge from the empty tree).
// Used by `merge-tree`.
pub fn merge_base_tree(ours: &str, theirs: &str, allow_unrelated: bool) -> anyhow::Result<(BTreeMap<String, Entry>, Vec<String>)> {
    let bases = find_all_merge_bases(ours, theirs)?;
    if bases.is_empty() {
        if !allow_unrelated {
            anyhow::bail!("fatal: refusing to merge unrelated histories");
        }
        return Ok((BTreeMap::new(), bases));
    }
    Ok((virtual_base(&bases, 1)?, bases))
}

fn virtual_base(bases: &[String], depth: usize) -> anyhow::Result<BTreeMap<String, Entry>> {
    let mut tree = resolve_tree_from_source(&bases[0])?;
    let mut previous = bases[0].clone();
    for next in &bases[1..] {
        let inner_bases = find_all_merge_bases(&previous, next)?;
        let inner_base = if inner_bases.is_empty() { BTreeMap::new() } else { virtual_base(&inner_bases, depth + 1)? };
        let next_tree = resolve_tree_from_source(next)?;
        tree = merge_trees_in_memory(&inner_base, &tree, &next_tree, "Temporary merge branch 1", "Temporary merge branch 2", 7 + 2 * depth)?.tree;
        previous = next.clone();
    }
    Ok(tree)
}

