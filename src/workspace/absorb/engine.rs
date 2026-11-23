//! Helpers of `rgit absorb`: staged hunks against HEAD, the commits that can be rewritten,
//! blame-based placement and rebuilding a file from a subset of its hunks.

use std::collections::HashMap;

use anyhow::Result;

use crate::core::commit::parse_commit_object;
use crate::diff::types::DiffOp;
use crate::workspace::absorb::types::{Placement, StagedHunk};

/// A blob's lines, each with its line ending (the last one may have none).
// Used by the `absorb` command.
pub fn lines_of(content: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(content).split_inclusive('\n').map(str::to_string).collect()
}

/// The hunks that turn `old` into `new`, with no context lines: every run of deleted and/or
/// inserted lines between two unchanged ones is one hunk.
// Used by the `absorb` command.
pub fn hunks_of(path: &str, old: &[String], new: &[String]) -> Vec<StagedHunk> {
    let old_refs: Vec<&str> = old.iter().map(String::as_str).collect();
    let new_refs: Vec<&str> = new.iter().map(String::as_str).collect();
    let mut hunks = Vec::new();
    let mut current: Option<StagedHunk> = None;
    let mut old_pos = 0;
    for op in crate::diff::myers::myers_diff(&old_refs, &new_refs) {
        match op {
            DiffOp::Keep(_) => {
                hunks.extend(current.take());
                old_pos += 1;
            }
            DiffOp::Delete(_) => {
                current.get_or_insert_with(|| StagedHunk { path: path.to_string(), old_start: old_pos, old_len: 0, new_lines: Vec::new() }).old_len += 1;
                old_pos += 1;
            }
            DiffOp::Insert(line) => {
                current.get_or_insert_with(|| StagedHunk { path: path.to_string(), old_start: old_pos, old_len: 0, new_lines: Vec::new() }).new_lines.push(line);
            }
        }
    }
    hunks.extend(current);
    hunks
}

/// The commits `absorb` may rewrite: HEAD and its first parents, newest first, up to (not
/// including) the first merge, the root commit, or (unless `force`) a commit a
/// remote-tracking branch already has.
// Used by the `absorb` command.
pub fn rewritable_commits(head: &str, force: bool) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut current = head.to_string();
    loop {
        let commit = parse_commit_object(&current)?;
        if commit.parents.len() != 1 || (!force && crate::workspace::fixup::engine::pushed_on(&current)?.is_some()) {
            break;
        }
        out.push(current.clone());
        current = commit.parents[0].clone();
    }
    Ok(out)
}

/// Decides where a hunk goes: into the commit that last changed every HEAD line it replaces,
/// when that is one rewritable commit; otherwise it stays staged, with the reason.
// Used by the `absorb` command.
pub fn place(hunk: &StagedHunk, owners: &[String], rewritable: &[String]) -> Placement {
    if hunk.old_len == 0 {
        return Placement::Left("only adds lines (nothing to blame)".to_string());
    }
    let mut commits: Vec<&String> = owners[hunk.old_start..hunk.old_start + hunk.old_len].iter().collect();
    commits.dedup();
    commits.sort();
    commits.dedup();
    if commits.len() > 1 {
        return Placement::Left(format!("changes lines from {} different commits", commits.len()));
    }
    let owner = commits[0];
    if rewritable.contains(owner) {
        return Placement::Into(owner.clone());
    }
    let why = match crate::workspace::fixup::engine::pushed_on(owner) {
        Ok(Some(remote)) => format!("its lines come from {}, which {} already has (--force rewrites it)", &owner[..7], remote),
        _ => format!("its lines come from {}, behind a merge or the root commit", &owner[..7]),
    };
    Placement::Left(why)
}

/// `path:3`, `path:10-12` (HEAD line numbers), or `path:+41` (lines added after line 41).
// Used by the `absorb` command's plan.
pub fn label(hunk: &StagedHunk) -> String {
    match hunk.old_len {
        0 => format!("{}:+{}", hunk.path, hunk.old_start),
        1 => format!("{}:{}", hunk.path, hunk.old_start + 1),
        n => format!("{}:{}-{}", hunk.path, hunk.old_start + 1, hunk.old_start + n),
    }
}

/// HEAD's lines with some of their (non-overlapping) hunks applied.
// Used by the `absorb` command to build the fixup commits.
pub fn apply_hunks(old: &[String], hunks: &[&StagedHunk]) -> String {
    let mut sorted: Vec<&&StagedHunk> = hunks.iter().collect();
    sorted.sort_by_key(|h| h.old_start);
    let mut out = String::new();
    let mut pos = 0;
    for hunk in sorted {
        for line in &old[pos..hunk.old_start] {
            out.push_str(line);
        }
        for line in &hunk.new_lines {
            out.push_str(line);
        }
        pos = hunk.old_start + hunk.old_len;
    }
    for line in &old[pos..] {
        out.push_str(line);
    }
    out
}

/// Short hash + subject of every commit named in `hashes`.
// Used by the `absorb` command's plan.
pub fn describe(hashes: &[String]) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for h in hashes {
        let subject = parse_commit_object(h)?.message.lines().next().unwrap_or("").to_string();
        out.insert(h.clone(), format!("{} {}", &h[..7], subject));
    }
    Ok(out)
}

/// Whether folding `hunk` into `target` would conflict: the fix merged onto the target's
/// version of the file (base: HEAD's), and the later commits' changes (target -> HEAD) merged
/// back on top of that. Adjacent edits count as conflicts, as in a rebase.
// Used by the `absorb` command, before anything is rewritten.
pub fn would_conflict(hunk: &StagedHunk, head_lines: &[String], target: &str) -> Result<bool> {
    use crate::merge::merge_tree::engine::merge_file_lines;
    let Some(blob) = crate::history::blame::engine::blob_hash_at_path(target, &hunk.path)? else {
        return Ok(true);
    };
    let at_target = String::from_utf8_lossy(&crate::core::object_store::read_object(&blob)?.1).to_string();
    let head: String = head_lines.concat();
    let fixed_head = apply_hunks(head_lines, &[hunk]);
    let fixed_target = merge_file_lines(&head, &at_target, &fixed_head, "target", "fix", 7);
    if !fixed_target.conflicts.is_empty() {
        return Ok(true);
    }
    let fixed_target = String::from_utf8_lossy(&fixed_target.content).to_string();
    Ok(!merge_file_lines(&at_target, &fixed_target, &head, "fixed", "later", 7).conflicts.is_empty())
}
