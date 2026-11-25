//! Helpers of `rgit stats`: the per-commit counts, lines per language at HEAD, and the
//! sparkline and bar renderings.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit};
use crate::history::stats::types::CommitStat;

type Tree = BTreeMap<String, ([u8; 20], u32)>;

/// `+0200` as seconds east of UTC (0 when it doesn't parse).
// Used by `collect_commits`.
fn tz_offset(tz: &str) -> i64 {
    let (sign, digits) = match tz.as_bytes().first() {
        Some(b'-') => (-1, &tz[1..]),
        Some(b'+') => (1, &tz[1..]),
        _ => return 0,
    };
    let hours: i64 = digits.get(..2).and_then(|h| h.parse().ok()).unwrap_or(0);
    let minutes: i64 = digits.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

/// The non-merge commits reachable from `head` (author time at or after `since`) that change a
/// file under `specs` (any file when empty), newest first, with their line and file counts.
// Used by the `stats` command.
pub fn collect_commits(head: &str, specs: &[String], since: Option<i64>) -> Result<Vec<CommitStat>> {
    let opts = crate::history::rev_list::engine::parse_revision_args(&[head.to_string()])?;
    let (walked, _) = crate::history::rev_list::engine::select_rev_list_commits(&opts)?;
    let wanted = |p: &str| specs.is_empty() || crate::workspace::ls_files::engine::matches_ls_files_pathspec(p, specs);
    let mut out = Vec::new();
    for w in walked {
        let commit = &w.commit;
        if commit.parents.len() > 1 || since.is_some_and(|s| commit.author.timestamp < s) {
            continue;
        }
        let mut old_tree = Tree::new();
        if let Some(parent) = commit.parents.first() {
            flatten_tree(&tree_hash_of_commit(parent)?, "", &mut old_tree)?;
        }
        let mut new_tree = Tree::new();
        flatten_tree(&commit.tree, "", &mut new_tree)?;
        let mut paths: Vec<&String> = old_tree.keys().chain(new_tree.keys()).filter(|p| old_tree.get(*p) != new_tree.get(*p) && wanted(p)).collect();
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            continue;
        }
        let (mut added, mut removed) = (0, 0);
        for p in &paths {
            let (a, r) = crate::history::hotspots::engine::line_changes(old_tree.get(*p).map(|e| e.0), new_tree.get(*p).map(|e| e.0))?;
            added += a;
            removed += r;
        }
        out.push(CommitStat {
            hash: commit.hash.clone(),
            subject: commit.message.lines().next().unwrap_or("").to_string(),
            author: commit.author.name.clone(),
            timestamp: commit.author.timestamp,
            offset: tz_offset(&commit.author.tz),
            added,
            removed,
            files: paths.len(),
        });
    }
    Ok(out)
}

/// The language of a file, from its name or extension (`Other (.ext)` for unknown ones).
// Used by `languages`.
pub fn language_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let known = match name {
        "Makefile" | "makefile" => Some("Makefile"),
        "Dockerfile" => Some("Dockerfile"),
        "Cargo.lock" => Some("TOML"),
        _ => None,
    };
    if let Some(lang) = known {
        return lang.to_string();
    }
    let Some((_, ext)) = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()) else {
        return "(no extension)".to_string();
    };
    let lang = match ext.to_ascii_lowercase().as_str() {
        "rs" => "Rust",
        "py" => "Python",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JSX",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TSX",
        "svelte" => "Svelte",
        "vue" => "Vue",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "Sass",
        "c" | "h" => "C",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "C++",
        "cs" => "C#",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "go" => "Go",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "sh" | "bash" | "zsh" => "Shell",
        "ps1" => "PowerShell",
        "sql" => "SQL",
        "md" | "markdown" => "Markdown",
        "txt" => "Text",
        "toml" => "TOML",
        "json" => "JSON",
        "yml" | "yaml" => "YAML",
        "xml" => "XML",
        "snap" => "Snapshot",
        other => return format!("Other (.{})", other),
    };
    lang.to_string()
}

/// Lines and files per language at `head`, for files under `specs` (binary files skipped).
// Used by the `stats` command.
pub fn languages(head: &str, specs: &[String]) -> Result<BTreeMap<String, (usize, usize)>> {
    let mut tree = Tree::new();
    flatten_tree(&tree_hash_of_commit(head)?, "", &mut tree)?;
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (path, (hash, mode)) in &tree {
        if *mode == 0o160000 || !(specs.is_empty() || crate::workspace::ls_files::engine::matches_ls_files_pathspec(path, specs)) {
            continue;
        }
        let content = read_object(&hex::encode(hash))?.1;
        if content.contains(&0) {
            continue;
        }
        let lines = content.split(|b| *b == b'\n').count() - usize::from(content.ends_with(b"\n") || content.is_empty());
        let entry = out.entry(language_of(path)).or_default();
        entry.0 += lines;
        entry.1 += 1;
    }
    Ok(out)
}

/// A sparkline: one block character per value, scaled to the largest.
// Used by the `stats` command's activity line.
pub fn sparkline(values: &[usize]) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().copied().max().unwrap_or(0);
    values
        .iter()
        .map(|&v| match v {
            0 => ' ',
            v => BLOCKS[((v * 8).div_ceil(max.max(1))).clamp(1, 8) - 1],
        })
        .collect()
}

/// A bar of up to `width` blocks for `value` out of `max` (at least one block when not zero).
// Used by the `stats` command's tables.
pub fn bar(value: usize, max: usize, width: usize) -> String {
    if value == 0 || max == 0 {
        return String::new();
    }
    "█".repeat((value * width).div_ceil(max).clamp(1, width))
}
