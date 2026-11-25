//! Helpers of `rgit story`: walking a path's history (following renames for a file), counting
//! the lines each commit changed, and who wrote the lines there today (blame).

use std::collections::BTreeMap;

use anyhow::Result;

use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit};
use crate::diff::types::DiffOp;
use crate::history::follow::follow_commit_pairs;
use crate::history::story::types::Event;

/// Whether `path` is a directory in HEAD's tree (some file is under `path/`).
// Used by the `story` command.
pub fn is_directory(head: &str, path: &str) -> Result<bool> {
    let mut tree = BTreeMap::new();
    flatten_tree(&tree_hash_of_commit(head)?, "", &mut tree)?;
    let prefix = format!("{}/", path);
    Ok(tree.keys().any(|p| p.starts_with(&prefix)))
}

/// A blob's lines (None for a binary blob or no blob).
// Used by `walk_history`.
fn lines(entry: Option<([u8; 20], u32)>) -> Result<Option<Vec<String>>> {
    let Some((hash, _)) = entry else { return Ok(Some(Vec::new())) };
    let content = read_object(&hex::encode(hash))?.1;
    if content.contains(&0) {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&content).split_inclusive('\n').map(str::to_string).collect()))
}

/// The commits reachable from `head` that changed `path`, newest first (merges skipped, like
/// `log --follow`). For a file, a rename or copy that created it is followed to the old name;
/// a directory is matched as a path prefix.
// Used by the `story` command.
pub fn walk_history(head: &str, path: &str, directory: bool) -> Result<Vec<Event>> {
    let opts = crate::history::rev_list::engine::parse_revision_args(&[head.to_string()])?;
    let (walked, _) = crate::history::rev_list::engine::select_rev_list_commits(&opts)?;
    let mut current = path.to_string();
    let mut events = Vec::new();
    for w in walked {
        let commit = &w.commit;
        if commit.parents.len() > 1 {
            continue;
        }
        let (pairs, renamed_from) = follow_commit_pairs(commit, &current)?;
        if pairs.is_empty() {
            continue;
        }
        let (mut added, mut removed, mut old_lines) = (Some(0usize), Some(0usize), 0usize);
        for pair in &pairs {
            match (lines(pair.old)?, lines(pair.new)?) {
                (Some(old), Some(new)) => {
                    old_lines += old.len();
                    let (o, n): (Vec<&str>, Vec<&str>) = (old.iter().map(String::as_str).collect(), new.iter().map(String::as_str).collect());
                    for op in crate::diff::myers::myers_diff(&o, &n) {
                        match op {
                            DiffOp::Insert(_) => *added.get_or_insert(0) += 1,
                            DiffOp::Delete(_) => *removed.get_or_insert(0) += 1,
                            DiffOp::Keep(_) => {}
                        }
                    }
                }
                _ => (added, removed) = (None, None),
            }
        }
        let renamed = pairs.iter().find(|p| p.rename.is_some()).map(|p| (p.old_path.clone(), p.new_path.clone()));
        let single = !directory && pairs.len() == 1;
        events.push(Event {
            commit: commit.hash.clone(),
            author: commit.author.name.clone(),
            timestamp: commit.author.timestamp,
            subject: commit.message.lines().next().unwrap_or("").to_string(),
            added,
            removed,
            old_lines,
            created: single && pairs[0].old.is_none(),
            deleted: single && pairs[0].new.is_none(),
            renamed,
        });
        if !directory && let Some(old) = renamed_from {
            current = old;
        }
    }
    Ok(events)
}

/// Who wrote the lines of `path` at `head` (blame), as author -> line count; every file under
/// it for a directory (binary files skipped).
// Used by the `story` command.
pub fn ownership(head: &str, path: &str, directory: bool) -> Result<BTreeMap<String, usize>> {
    let mut files: Vec<String> = Vec::new();
    let mut tree = BTreeMap::new();
    flatten_tree(&tree_hash_of_commit(head)?, "", &mut tree)?;
    let prefix = format!("{}/", path);
    for (p, (hash, mode)) in &tree {
        if (p == path || (directory && p.starts_with(&prefix))) && *mode != 0o160000 && !read_object(&hex::encode(hash))?.1.contains(&0) {
            files.push(p.clone());
        }
    }
    let mut owners: BTreeMap<String, usize> = BTreeMap::new();
    for file in files {
        for line in crate::history::blame::engine::blame_file(&file, head, false, None)? {
            *owners.entry(line.author).or_default() += 1;
        }
    }
    Ok(owners)
}

/// `2023-11-14` for a Unix timestamp (UTC).
// Used by the `story` command.
pub fn day(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{:04}-{:02}-{:02}", y, m, d)
}
