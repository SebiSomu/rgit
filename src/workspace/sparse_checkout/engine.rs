use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::core::config::get_config_value;
use crate::workspace::index_plumbing::{worktree_state, write_entry_to_worktree};
use crate::core::wildmatch::{wildmatch, WM_PATHNAME};
use crate::core::index::{build_entry, read_index, write_index, IndexEntry};
use crate::workspace::types::IndexFileState;

/// Usage text of `sparse-checkout`.
pub const SPARSE_CHECKOUT_USAGE: &str = "usage: rgit sparse-checkout (init | list | set | add | reapply | disable | check-rules) [<options>]\n\n   init [--cone | --no-cone]\n   set [--cone | --no-cone] [--stdin | <patterns>...]\n   add [--stdin | <patterns>...]\n   list\n   reapply [--cone | --no-cone]\n   disable\n   check-rules [-z] [--cone | --no-cone] [--rules-file <file>]\n";

/// Path of the sparse-checkout pattern file (per worktree).
// Used by `sparse-checkout` and `reapply_sparse_checkout`.
pub fn sparse_patterns_path() -> PathBuf {
    crate::core::repo::git_dir().join("info").join("sparse-checkout")
}

/// Whether `core.sparseCheckout` is on.
// Used by `sparse-checkout` and by `cli.rs` after commands that rewrite the working tree.
pub fn sparse_checkout_enabled() -> bool {
    matches!(get_config_value("core.sparsecheckout").ok().flatten().as_deref(), Some("true" | "yes" | "on" | "1"))
}

/// Whether `core.sparseCheckoutCone` is on.
// Used by `sparse-checkout`.
pub fn sparse_cone_mode() -> bool {
    matches!(get_config_value("core.sparsecheckoutcone").ok().flatten().as_deref(), Some("true" | "yes" | "on" | "1"))
}

/// The pattern lines of the sparse-checkout file, without blank lines and comments.
// Used by `sparse-checkout` and `reapply_sparse_checkout`.
pub fn read_sparse_patterns() -> Vec<String> {
    fs::read_to_string(sparse_patterns_path())
        .map(|text| text.lines().map(|l| l.trim_end().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect())
        .unwrap_or_default()
}

/// Tests one pattern against one path the way `.gitignore` patterns work: a trailing `/`
/// only matches directories, a pattern with a slash is anchored at the top, one without
/// matches the last path component at any depth.
// Used by `path_in_sparse_checkout`.
fn pattern_matches(pattern: &str, path: &str, is_dir: bool) -> bool {
    let mut pattern = pattern;
    if let Some(stripped) = pattern.strip_suffix('/') {
        if !is_dir {
            return false;
        }
        pattern = stripped;
    }
    if let Some(anchored) = pattern.strip_prefix('/') {
        return wildmatch(anchored, path, WM_PATHNAME);
    }
    if pattern.contains('/') {
        return wildmatch(pattern, path, WM_PATHNAME);
    }
    wildmatch(pattern, path.rsplit('/').next().unwrap_or(path), 0)
}

/// Whether a file belongs to the sparse checkout described by `patterns`: the last pattern
/// that matches the file decides (`!` excludes); when none matches, the same question is
/// asked for its directory, then that directory's parent, and so on. A file nothing matches
/// is outside. This one rule covers both free-form patterns and the cone-mode pattern
/// files (`/*`, `!/*/`, `/dir/`, `!/dir/*/`, ...).
// Used by `reapply_sparse_checkout` and `sparse-checkout check-rules`.
pub fn path_in_sparse_checkout(path: &str, patterns: &[String]) -> bool {
    let mut current = path;
    let mut is_dir = false;
    loop {
        let mut decision: Option<bool> = None;
        for pattern in patterns {
            let (negated, body) = match pattern.strip_prefix('!') {
                Some(body) => (true, body),
                None => (false, pattern.strip_prefix('\\').filter(|rest| rest.starts_with(['!', '#'])).unwrap_or(pattern)),
            };
            if pattern_matches(body, current, is_dir) {
                decision = Some(!negated);
            }
        }
        if let Some(included) = decision {
            return included;
        }
        match current.rsplit_once('/') {
            Some((parent, _)) => {
                current = parent;
                is_dir = true;
            }
            None => return false,
        }
    }
}

/// The pattern file for cone mode: all files at the top level, then for every listed
/// directory its own files and everything below it, and for each of its parent directories
/// only the files directly inside.
// Used by `sparse-checkout set` / `add` / `init`.
pub fn cone_patterns(directories: &[String]) -> String {
    let mut recursive: BTreeSet<String> = directories.iter().map(|d| d.trim_matches('/').to_string()).filter(|d| !d.is_empty()).collect();
    // A directory below another listed directory adds nothing.
    let all = recursive.clone();
    recursive.retain(|dir| !all.iter().any(|other| dir.starts_with(&format!("{}/", other))));
    let mut parents: BTreeSet<String> = BTreeSet::new();
    for dir in &recursive {
        let mut current = dir.as_str();
        while let Some((parent, _)) = current.rsplit_once('/') {
            parents.insert(parent.to_string());
            current = parent;
        }
    }
    let mut out = String::from("/*\n!/*/\n");
    for parent in parents.iter().filter(|p| !recursive.contains(*p)) {
        out.push_str(&format!("/{}/\n!/{}/*/\n", parent, parent));
    }
    for dir in &recursive {
        out.push_str(&format!("/{}/\n", dir));
    }
    out
}

/// The directories a cone-mode pattern file selects recursively (what `list` prints):
/// every `/dir/` line that is not followed by its `!/dir/*/` companion.
// Used by `sparse-checkout list` / `add`.
pub fn cone_directories(patterns: &[String]) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    for pattern in patterns {
        let Some(dir) = pattern.strip_prefix('/').and_then(|p| p.strip_suffix('/')) else { continue };
        if dir.is_empty() || dir == "*" {
            continue;
        }
        if !patterns.contains(&format!("!/{}/*/", dir)) {
            dirs.push(dir.to_string());
        }
    }
    dirs.sort();
    dirs
}

/// Whether a pattern file has the shape cone mode writes.
// Used by `sparse-checkout list` / `add` / `check-rules`.
pub fn is_cone_pattern_file(patterns: &[String]) -> bool {
    patterns.iter().all(|p| p == "/*" || p == "!/*/" || (p.starts_with('/') && p.ends_with('/')) || (p.starts_with("!/") && p.ends_with("/*/")))
}

/// Brings the working tree and the index's skip-worktree bits in line with the patterns
/// (`None` = sparse checkout off, every file wanted). A file that should be present is
/// written if it is missing; a file that should be absent is removed if it is unchanged
/// and its entry marked skip-worktree. A file that should be absent but has local changes
/// is left alone and returned.
// Used by `sparse-checkout` and by `cli.rs` after commands that rewrite the working tree.
pub fn reapply_sparse_checkout(patterns: Option<&[String]>) -> Result<Vec<String>> {
    let mut entries: Vec<IndexEntry> = read_index().unwrap_or_default();
    let mut left: Vec<String> = Vec::new();
    let mut changed = false;
    for entry in entries.iter_mut() {
        if entry.mode & 0o170000 == 0o160000 {
            continue;
        }
        let wanted = patterns.is_none_or(|patterns| path_in_sparse_checkout(&entry.path, patterns));
        let path = Path::new(&entry.path).to_path_buf();
        if wanted {
            if !entry.skip_worktree {
                continue;
            }
            entry.skip_worktree = false;
            changed = true;
            if !path.exists() {
                write_entry_to_worktree(entry, &path, false).map_err(|message| anyhow::anyhow!(message))?;
                if let Ok(metadata) = fs::metadata(&path) {
                    let mode = entry.mode;
                    *entry = build_entry(&entry.path, entry.hash, &metadata);
                    entry.mode = mode;
                }
            }
            continue;
        }
        if entry.skip_worktree && !path.exists() {
            continue;
        }
        // Compare without the bit, so that a file that is still there is really looked at.
        let mut probe = entry.clone();
        probe.skip_worktree = false;
        match worktree_state(&probe) {
            IndexFileState::Modified(_) => {
                left.push(entry.path.clone());
                if entry.skip_worktree {
                    entry.skip_worktree = false;
                    changed = true;
                }
            }
            state => {
                if state == IndexFileState::Clean {
                    fs::remove_file(&path)?;
                    let mut parent = path.parent();
                    while let Some(dir) = parent.filter(|d| !d.as_os_str().is_empty()) {
                        if fs::remove_dir(dir).is_err() {
                            break;
                        }
                        parent = dir.parent();
                    }
                }
                if !entry.skip_worktree {
                    entry.skip_worktree = true;
                    changed = true;
                }
            }
        }
    }
    if changed {
        write_index(&mut entries)?;
    }
    Ok(left)
}

/// The set of index paths that are marked skip-worktree and absent from the working tree:
/// to every comparison of the working tree with the index they count as unchanged.
// Used by `status`, `diff`, `commit -a`, `stash` and the clean-tree checks.
pub fn skipped_worktree_paths(entries: &[IndexEntry]) -> BTreeSet<String> {
    entries.iter().filter(|e| e.skip_worktree && !Path::new(&e.path).exists()).map(|e| e.path.clone()).collect()
}

/// Commands after which the working tree may hold files outside the sparse checkout (or
/// entries may have lost their skip-worktree bit), so the patterns are applied again.
const WORKTREE_COMMANDS: [&str; 22] = [
    "checkout", "switch", "reset", "merge", "pull", "rebase", "cherry-pick", "revert", "stash", "restore", "am", "apply", "read-tree",
    "commit", "add", "stage", "rm", "mv", "bisect", "checkout-index", "clone", "worktree",
];

/// Re-applies the sparse-checkout patterns when the command that just ran is one that
/// rewrites the working tree or the index and `core.sparseCheckout` is on. Files with local
/// changes are left alone silently.
// Used by `main` after every successful command.
pub fn reapply_after_command() -> Result<()> {
    let command = std::env::args().nth(1).unwrap_or_default();
    if !WORKTREE_COMMANDS.contains(&command.as_str()) || !crate::core::repo::exists() || !sparse_checkout_enabled() {
        return Ok(());
    }
    reapply_sparse_checkout(Some(&read_sparse_patterns()))?;
    Ok(())
}
