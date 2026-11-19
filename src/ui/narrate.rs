use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use crate::core::commit::commit_parents;
use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit};
use crate::merge::conflicts::types::ConflictHunk;
use crate::ui::types::RepoSnapshot;
use crate::core::refs;

type TreeMap = BTreeMap<String, ([u8; 20], u32)>;

/// The operation a command left in progress (`merge`, `rebase`, `cherry-pick`, `revert`,
/// `bisect`), read from the state files under `.git`.
// Used by `take_snapshot`, the narration in ui.rs and `rgit conflicts`.
pub fn operation_in_progress() -> Option<String> {
    let exists = |p: &str| crate::core::repo::path(p).exists();
    if crate::merge::rebase::engine::rebase_in_progress() {
        Some("rebase".into())
    } else if exists(".git/MERGE_HEAD") {
        Some("merge".into())
    } else if exists(".git/CHERRY_PICK_HEAD") {
        Some("cherry-pick".into())
    } else if exists(".git/REVERT_HEAD") {
        Some("revert".into())
    } else if exists(crate::history::bisect::engine::BISECT_START_PATH) {
        Some("bisect".into())
    } else if exists(STASH_CONFLICT_PATH) && (exists(CONFLICTS_LIST_PATH) || crate::core::index::read_index().unwrap_or_default().iter().any(|e| e.stage != 0)) {
        // `stash apply` that stopped on conflicts: git keeps no state file for it.
        Some("stash".into())
    } else {
        None
    }
}

/// The flattened tree of a commit (empty for None or an unreadable commit).
// Used by the narration helpers below.
pub fn commit_tree_map(commit: Option<&str>) -> TreeMap {
    let mut map = BTreeMap::new();
    if let Some(c) = commit {
        if let Ok(tree) = tree_hash_of_commit(c) {
            let _ = flatten_tree(&tree, "", &mut map);
        }
    }
    map
}

/// Captures what rgit narrates about: HEAD, every ref, the index (stage 0 and unmerged
/// stages), what is staged against HEAD, the stash count and the operation in progress; with
/// `worktree`, also the tracked files changed in the working tree and the untracked files.
// Used by `ui::begin` and the exit-time report.
pub fn take_snapshot(worktree: bool) -> RepoSnapshot {
    let head_state = refs::resolve_head().ok();
    let head_branch = match &head_state {
        Some(refs::HeadState::Branch(b)) => Some(b.clone()),
        _ => None,
    };
    let head = refs::resolve_head_commit().ok().flatten();
    let refs: BTreeMap<String, String> = crate::references::for_each_ref::engine::refs::collect_ref_items(None).into_iter().map(|i| (i.refname, i.oid)).collect();
    let entries = crate::core::index::read_index().unwrap_or_default();
    let mut index = BTreeMap::new();
    let mut unmerged: BTreeMap<String, u8> = BTreeMap::new();
    for e in &entries {
        if e.stage == 0 {
            index.insert(e.path.clone(), e.hash);
        } else {
            *unmerged.entry(e.path.clone()).or_default() |= 1 << (e.stage - 1);
        }
    }
    let head_tree = commit_tree_map(head.as_deref());
    let mut staged: Vec<String> = Vec::new();
    for (path, hash) in &index {
        if head_tree.get(path).map(|e| &e.0) != Some(hash) {
            staged.push(path.clone());
        }
    }
    for path in head_tree.keys() {
        if !index.contains_key(path) && !unmerged.contains_key(path) {
            staged.push(path.clone());
        }
    }
    staged.sort();
    let (worktree_changes, untracked) = if worktree {
        let changes = crate::core::commit::tracked_worktree_changes().unwrap_or_default();
        let tracked: BTreeSet<String> = entries.iter().map(|e| e.path.clone()).collect();
        let untracked = crate::workspace::state_files::collect_untracked_files(&tracked, false).unwrap_or_default();
        (Some(changes), Some(untracked))
    } else {
        (None, None)
    };
    RepoSnapshot {
        head_branch,
        head,
        refs,
        index,
        unmerged,
        staged,
        stash_count: crate::workspace::state_files::read_stash_list().map(|l| l.len()).unwrap_or(0),
        operation: operation_in_progress(),
        worktree_changes,
        untracked,
    }
}

/// Counts the commits reachable from `new` but not from `old` (all of `new`'s history when
/// `old` is None).
// Used by the narration of commit, merge, fetch, push, cherry-pick and switch.
pub fn commits_between(old: Option<&str>, new: &str) -> usize {
    let new_set = crate::core::commit::collect_ancestors(new).unwrap_or_default();
    let old_set: HashSet<String> = match old {
        Some(o) => crate::core::commit::collect_ancestors(o).unwrap_or_default(),
        None => HashSet::new(),
    };
    new_set.difference(&old_set).count()
}

/// Whether `ancestor` is reachable from `descendant` (a commit counts as its own ancestor).
// Used by the narration (fast-forward / already-up-to-date / force-push checks).
pub fn is_ancestor(ancestor: &str, descendant: &str) -> bool {
    ancestor == descendant || crate::core::commit::is_reachable(descendant, ancestor).unwrap_or(false)
}

/// What changed between two commits' trees: (added, modified, deleted) paths and the
/// added / removed line counts of the text files among them (files over 1 MiB are skipped).
// Used by the commit, merge and pull summaries.
pub fn tree_changes(old: Option<&str>, new: Option<&str>) -> (Vec<String>, Vec<String>, Vec<String>, usize, usize) {
    let a = commit_tree_map(old);
    let b = commit_tree_map(new);
    let (mut added, mut modified, mut deleted) = (Vec::new(), Vec::new(), Vec::new());
    let (mut plus, mut minus) = (0usize, 0usize);
    let blob = |h: &[u8; 20]| read_object(&hex::encode(h)).map(|(_, c)| c).unwrap_or_default();
    let mut count = |old: Option<&[u8; 20]>, new: Option<&[u8; 20]>| {
        let o = old.map(blob).unwrap_or_default();
        let n = new.map(blob).unwrap_or_default();
        if o.len() > 1 << 20 || n.len() > 1 << 20 || o.contains(&0) || n.contains(&0) {
            return;
        }
        let (os, ns) = (String::from_utf8_lossy(&o), String::from_utf8_lossy(&n));
        let (ol, nl): (Vec<&str>, Vec<&str>) = (os.lines().collect(), ns.lines().collect());
        for r in diff::slice(&ol, &nl) {
            match r {
                diff::Result::Left(_) => minus += 1,
                diff::Result::Right(_) => plus += 1,
                diff::Result::Both(..) => {}
            }
        }
    };
    for (path, (hash, _)) in &b {
        match a.get(path) {
            None => {
                added.push(path.clone());
                count(None, Some(hash));
            }
            Some((old_hash, _)) if old_hash != hash => {
                modified.push(path.clone());
                count(Some(old_hash), Some(hash));
            }
            _ => {}
        }
    }
    for (path, (hash, _)) in &a {
        if !b.contains_key(path) {
            deleted.push(path.clone());
            count(Some(hash), None);
        }
    }
    (added, modified, deleted, plus, minus)
}

/// Looks up the blob id of `path` inside the tree `tree_hash`, walking one tree object per
/// path component.
// Used by `last_change`.
fn blob_at(tree_hash: &str, path: &str) -> Option<[u8; 20]> {
    let mut current = tree_hash.to_string();
    let parts: Vec<&str> = path.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        let (_, content) = read_object(&current).ok()?;
        let mut pos = 0;
        let mut found = None;
        while pos < content.len() {
            let space = content[pos..].iter().position(|&b| b == b' ')? + pos;
            let nul = content[space..].iter().position(|&b| b == 0)? + space;
            let name = &content[space + 1..nul];
            let mut id = [0u8; 20];
            id.copy_from_slice(content.get(nul + 1..nul + 21)?);
            if name == part.as_bytes() {
                found = Some(id);
                break;
            }
            pos = nul + 21;
        }
        let id = found?;
        if i + 1 == parts.len() {
            return Some(id);
        }
        current = hex::encode(id);
    }
    None
}

/// Subject, author name and author timestamp of a commit.
// Used by `last_change` and the conflict reports.
pub fn commit_info(hash: &str) -> Option<(String, String, i64)> {
    let (kind, content) = read_object(hash).ok()?;
    if kind != "commit" {
        return None;
    }
    let text = String::from_utf8_lossy(&content).to_string();
    let (headers, message) = text.split_once("\n\n").unwrap_or((&text, ""));
    let mut author = String::new();
    let mut when = 0i64;
    for line in headers.lines() {
        if let Some(rest) = line.strip_prefix("author ") {
            if let Some(lt) = rest.find(" <") {
                author = rest[..lt].to_string();
            }
            let fields: Vec<&str> = rest.rsplitn(3, ' ').collect();
            if fields.len() >= 2 {
                when = fields[1].parse().unwrap_or(0);
            }
        }
    }
    let subject = message.lines().next().unwrap_or("").to_string();
    Some((subject, author, when))
}

/// The newest commit on `tip`'s first-parent line that changed `path` (looks back at most
/// 500 commits): its hash, subject, author and time.
// Used by the conflict reports ("who changed it on each side").
pub fn last_change(tip: &str, path: &str) -> Option<(String, String, String, i64)> {
    let mut current = tip.to_string();
    for _ in 0..500 {
        let (_, content) = read_object(&current).ok()?;
        let text = String::from_utf8_lossy(&content).to_string();
        let tree = text.lines().next()?.strip_prefix("tree ")?.to_string();
        let here = blob_at(&tree, path);
        let parent = commit_parents(&text).into_iter().next();
        let before = parent.as_deref().and_then(|p| tree_hash_of_commit(p).ok()).and_then(|t| blob_at(&t, path));
        if here != before || parent.is_none() {
            if here.is_none() && before.is_none() {
                return None;
            }
            let (subject, author, when) = commit_info(&current)?;
            return Some((current, subject, author, when));
        }
        current = parent?;
    }
    None
}

/// "just now" / "5 minutes ago" / "2 days ago" for a Unix timestamp.
// Used by the conflict reports.
pub fn relative_time(ts: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(ts);
    let d = (now - ts).max(0);
    let unit = |n: i64, what: &str| format!("{} {}{} ago", n, what, if n == 1 { "" } else { "s" });
    match d {
        0..=59 => "just now".into(),
        60..=3599 => unit(d / 60, "minute"),
        3600..=86_399 => unit(d / 3600, "hour"),
        86_400..=2_591_999 => unit(d / 86_400, "day"),
        2_592_000..=31_535_999 => unit(d / 2_592_000, "month"),
        _ => unit(d / 31_536_000, "year"),
    }
}

/// Splits a file's text into its `<<<<<<<` / `|||||||` / `=======` / `>>>>>>>` conflict
/// hunks, with 1-based line numbers and the labels written after the markers.
// Used by the conflict reports, `add` (markers left in a resolved file) and `rgit conflicts`.
pub fn parse_conflict_hunks(text: &str) -> Vec<ConflictHunk> {
    let mut hunks = Vec::new();
    let mut current: Option<(ConflictHunk, u8)> = None; // 0 = ours, 1 = base, 2 = theirs
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if let Some(label) = line.strip_prefix("<<<<<<<") {
            current = Some((
                ConflictHunk { start: n, end: n, ours: Vec::new(), base: None, theirs: Vec::new(), ours_label: label.trim().to_string(), theirs_label: String::new() },
                0,
            ));
            continue;
        }
        let Some((hunk, part)) = current.as_mut() else { continue };
        if line.starts_with("|||||||") && *part == 0 {
            hunk.base = Some(Vec::new());
            *part = 1;
        } else if line == "=======" && *part < 2 {
            *part = 2;
        } else if let Some(label) = line.strip_prefix(">>>>>>>").filter(|_| *part == 2) {
            hunk.theirs_label = label.trim().to_string();
            hunk.end = n;
            hunks.push(current.take().unwrap().0);
        } else {
            match part {
                0 => hunk.ours.push(line.to_string()),
                1 => hunk.base.get_or_insert_with(Vec::new).push(line.to_string()),
                _ => hunk.theirs.push(line.to_string()),
            }
        }
    }
    hunks
}

/// The first line of `path` (1-based) that is a conflict marker, if the file has any.
// Used by the `add` and `commit` warnings.
pub fn first_marker_line(path: &str) -> Option<usize> {
    let content = fs::read(path).ok()?;
    if content.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(&content);
    parse_conflict_hunks(&text).first().map(|h| h.start)
}

/// git's conflict kind for an unmerged path, from which of stages 1-3 it has, in words.
// Used by the conflict reports and `rgit conflicts`.
pub fn conflict_kind(mask: u8) -> &'static str {
    match mask {
        1 => "both sides deleted it",
        2 => "added only on your side (rename/directory clash)",
        3 => "you changed it, they deleted it (modify/delete)",
        4 => "added only on their side (rename/directory clash)",
        5 => "you deleted it, they changed it (modify/delete)",
        6 => "both sides added a file with this name (add/add)",
        _ => "both sides edited the same lines",
    }
}

/// The closest name to `wanted` among `candidates` (same file name in another directory, or
/// within a small edit distance), for "did you mean" hints.
// Used by the `add`, `rm` and `restore` narration.
pub fn suggest(wanted: &str, candidates: &[String]) -> Option<String> {
    let base = wanted.rsplit('/').next().unwrap_or(wanted);
    if let Some(c) = candidates.iter().find(|c| c.rsplit('/').next() == Some(base) && c.as_str() != wanted) {
        return Some(c.clone());
    }
    let limit = (wanted.len() / 3).max(2);
    candidates.iter().map(|c| (levenshtein(wanted, c), c)).filter(|(d, _)| *d <= limit).min_by_key(|(d, _)| *d).map(|(_, c)| c.clone())
}

/// Edit distance between two strings (insert / delete / substitute, all cost 1).
// Used by `suggest`.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (row[j] + 1).min(row[j - 1] + 1).min(prev + usize::from(a[i - 1] != b[j - 1]));
            prev = cur;
        }
    }
    row[b.len()]
}

/// Risky things among newly staged or committed paths: likely secrets (by file name or
/// content), build output, and files over the size limit (`rgit.largeFileWarning` bytes,
/// default 50 MiB, GitHub's warning size). Returns (secrets, build output, large) as
/// (path, what) pairs.
// Used by the `add` and `commit` warnings.
pub fn scan_risks(paths: &[String]) -> (Vec<(String, String)>, Vec<String>, Vec<(String, u64)>) {
    let limit: u64 = crate::core::config::get_config_value("rgit.largeFileWarning").ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(50 << 20);
    let content_rules: Vec<(regex::Regex, &str)> = [
        (r"AKIA[0-9A-Z]{16}", "an AWS access key"),
        (r"-----BEGIN [A-Z ]*PRIVATE KEY-----", "a private key"),
        (r"gh[pousr]_[A-Za-z0-9]{36,}", "a GitHub token"),
        (r"xox[baprs]-[A-Za-z0-9-]{10,}", "a Slack token"),
        (r#"(?i)(api[_-]?key|secret|passwd|password|token)\s*[:=]\s*['"][^'"\s]{8,}['"]"#, "a hard-coded password or key"),
    ]
    .iter()
    .filter_map(|(re, what)| regex::Regex::new(re).ok().map(|r| (r, *what)))
    .collect();
    let (mut secrets, mut build, mut large) = (Vec::new(), Vec::new(), Vec::new());
    for path in paths {
        let name = path.rsplit('/').next().unwrap_or(path);
        let lower = name.to_ascii_lowercase();
        let Ok(meta) = fs::metadata(path) else { continue };
        if !meta.is_file() {
            continue;
        }
        if meta.len() > limit {
            large.push((path.clone(), meta.len()));
        }
        if lower == ".env" || lower.starts_with(".env.") && lower != ".env.example" || ["id_rsa", "id_ed25519", "id_ecdsa", "credentials.json"].contains(&lower.as_str()) || [".pem", ".p12", ".pfx", ".key"].iter().any(|e| lower.ends_with(e)) {
            secrets.push((path.clone(), "a secrets/key file".to_string()));
        } else if meta.len() <= 1 << 20 {
            if let Ok(content) = fs::read(path) {
                let text = String::from_utf8_lossy(&content);
                if let Some((_, what)) = content_rules.iter().find(|(re, _)| re.is_match(&text)) {
                    secrets.push((path.clone(), what.to_string()));
                }
            }
        }
        let dirs = ["target", "node_modules", "__pycache__", ".venv", "dist", "build"];
        let in_build_dir = path.split('/').rev().skip(1).any(|c| dirs.contains(&c));
        let build_ext = [".o", ".obj", ".class", ".pyc", ".exe", ".dll", ".so", ".dylib"].iter().any(|e| lower.ends_with(e));
        if in_build_dir || build_ext {
            build.push(path.clone());
        }
    }
    (secrets, build, large)
}

/// Where `rgit` keeps the list of files that conflicted when the current operation stopped,
/// so later commands can say "2 of 3 resolved".
pub const CONFLICTS_LIST_PATH: &str = ".git/RGIT_CONFLICTS";

/// Marks that a `stash apply` / `stash pop` stopped on conflicts (written by the stash code,
/// removed once the conflicts are resolved or thrown away).
pub const STASH_CONFLICT_PATH: &str = ".git/RGIT_STASH_CONFLICT";

/// Forgets a finished stash conflict: the marker and the recorded file list.
// Used by `stash`, `reset` and the "all resolved" progress line.
pub fn clear_stash_conflict() {
    let _ = fs::remove_file(crate::core::repo::path(STASH_CONFLICT_PATH));
    let _ = fs::remove_file(crate::core::repo::path(CONFLICTS_LIST_PATH));
}

/// Records the conflicted paths of the operation that just stopped.
// Used by the conflict reports.
pub fn save_conflict_list(paths: &[String]) {
    let _ = fs::write(crate::core::repo::path(CONFLICTS_LIST_PATH), paths.join("\n") + "\n");
}

/// The conflicted paths recorded when the current operation stopped (empty if none, or once
/// no operation is in progress, when the stale list is removed).
// Used by `status`, `add` and `rgit conflicts` progress lines.
pub fn load_conflict_list() -> Vec<String> {
    let path = crate::core::repo::path(CONFLICTS_LIST_PATH);
    if operation_in_progress().is_none() {
        let _ = fs::remove_file(&path);
        return Vec::new();
    }
    fs::read_to_string(path).map(|s| s.lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
}

/// The commit on "their" side of a stopped operation: MERGE_HEAD, CHERRY_PICK_HEAD,
/// REVERT_HEAD, or for a rebase the commit named in the file's `>>>>>>>` label.
// Used by the conflict reports and `rgit conflicts`.
pub fn their_commit(operation: &str, theirs_label: Option<&str>) -> Option<String> {
    let read = |p: &str| fs::read_to_string(crate::core::repo::path(p)).ok().map(|s| s.lines().next().unwrap_or("").trim().to_string()).filter(|s| !s.is_empty());
    match operation {
        "merge" => read(".git/MERGE_HEAD"),
        "cherry-pick" => read(".git/CHERRY_PICK_HEAD"),
        "revert" => read(".git/REVERT_HEAD"),
        _ => {
            let label = theirs_label?;
            let first = label.split_whitespace().next()?;
            crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", first)).ok()
        }
    }
}

/// Reads the working-tree file of a conflicted path and returns its hunks (empty when the file
/// is missing, binary or has no markers).
// Used by the conflict reports and `rgit conflicts`.
pub fn file_conflict_hunks(path: &str) -> Vec<ConflictHunk> {
    match fs::read(path) {
        Ok(content) if !content.contains(&0) => parse_conflict_hunks(&String::from_utf8_lossy(&content)),
        _ => Vec::new(),
    }
}

/// Whether `path` exists as a file or directory in the working tree.
// Used by the narration of `add`, `rm`, `checkout` (paths vs branches).
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Counts ignored files below `paths` (all of the working tree for `.`).
// Used by `add`'s "nothing to add" explanation.
pub fn ignored_files_under(paths: &[String]) -> Vec<String> {
    let tracked: BTreeSet<String> = crate::core::index::read_index().unwrap_or_default().into_iter().map(|e| e.path).collect();
    let all = crate::workspace::state_files::collect_untracked_files(&tracked, true).unwrap_or_default();
    let plain: BTreeSet<String> = crate::workspace::state_files::collect_untracked_files(&tracked, false).unwrap_or_default().into_iter().collect();
    all.into_iter().filter(|p| !plain.contains(p)).filter(|p| under_any(p, paths)).collect()
}

/// Whether `path` is one of `specs` or below one of them (`.` / empty matches everything).
// Used by the `add` / `rm` / `restore` narration.
pub fn under_any(path: &str, specs: &[String]) -> bool {
    specs.is_empty()
        || specs.iter().any(|s| {
            let s = s.trim_end_matches('/');
            s.is_empty() || s == "." || path == s || path.starts_with(&format!("{}/", s))
        })
}

/// `.gitignore` source, line and pattern that ignores `path`, if one does.
// Used by `add`'s "'x' is ignored" explanation.
pub fn ignore_reason(path: &str) -> Option<(String, usize, String)> {
    let mut cache = std::collections::HashMap::new();
    let is_dir = Path::new(path).is_dir();
    let m = crate::workspace::check_ignore::engine::find_ignore_match(path, is_dir, &mut cache)?;
    if m.pattern_text.starts_with('!') {
        return None;
    }
    Some((m.source, m.line_number, m.pattern_text))
}
