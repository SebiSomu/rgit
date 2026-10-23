use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::helpers::{hash_content, is_path_ignored, parse_gitignore_line};
use crate::index::IndexEntry;
use crate::objects::{GitIgnoreRule, UntrackedPath, WorktreeState};

/// Compares a tracked index entry against its working-tree file: `Deleted` when the file is
/// missing (or replaced by a directory), `Modified` when its blob hash differs from the index.
// Used by `ls-files` (`-m` / `-d`).
pub fn worktree_state(entry: &IndexEntry) -> anyhow::Result<WorktreeState> {
    let path = Path::new(&entry.path);
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(WorktreeState::Deleted),
    };
    if metadata.is_dir() {
        return Ok(WorktreeState::Deleted);
    }

    let content = fs::read(path).with_context(|| format!("Failed to read {}", entry.path))?;
    if hash_content("blob", &content) == entry.hash {
        Ok(WorktreeState::Unchanged)
    } else {
        Ok(WorktreeState::Modified)
    }
}

/// Returns every ancestor directory (forward-slash, relative) of the given tracked paths,
/// so a walker can tell which directories contain tracked content.
// Used by `ls-files` (`-o`) via `collect_untracked_paths`.
pub fn tracked_ancestor_dirs(tracked_paths: &BTreeSet<String>) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();
    for path in tracked_paths {
        let mut end = 0;
        while let Some(pos) = path[end..].find('/') {
            end += pos;
            dirs.insert(path[..end].to_string());
            end += 1;
        }
    }
    dirs
}

/// Walks the working tree (skipping `.git`) and returns every file not present in the index,
/// flagged as ignored when a `.gitignore` rule matches it or any ancestor directory.
/// Ignored directories without tracked content are only descended into when
/// `descend_ignored` is set. Untracked nested repositories are reported once as `dir/`.
// Used by `ls-files` (`-o`).
pub fn collect_untracked_paths(tracked_paths: &BTreeSet<String>, descend_ignored: bool) -> anyhow::Result<Vec<UntrackedPath>> {
    let tracked_dirs = tracked_ancestor_dirs(tracked_paths);
    let mut out = Vec::new();
    walk_untracked(Path::new("."), "", &[], false, tracked_paths, &tracked_dirs, descend_ignored, &mut out)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn walk_untracked(dir: &Path, rel: &str, parent_rules: &[GitIgnoreRule], dir_ignored: bool, tracked_paths: &BTreeSet<String>, tracked_dirs: &BTreeSet<String>, descend_ignored: bool, out: &mut Vec<UntrackedPath>) -> anyhow::Result<()> {
    let mut rules = parent_rules.to_vec();
    if !dir_ignored {
        rules.extend(read_gitignore_rules(dir, rel));
    }

    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let child_rel = if rel.is_empty() { name } else { format!("{}/{}", rel, name) };
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };

        if file_type.is_dir() {
            let ignored = dir_ignored || is_path_ignored(&rules, &child_rel, true);
            let has_tracked = tracked_dirs.contains(&child_rel);
            if ignored && !descend_ignored && !has_tracked {
                continue;
            }
            if !has_tracked && entry.path().join(".git").exists() {
                out.push(UntrackedPath { path: format!("{}/", child_rel), ignored });
                continue;
            }
            walk_untracked(&entry.path(), &child_rel, &rules, ignored, tracked_paths, tracked_dirs, descend_ignored, out)?;
        } else {
            if tracked_paths.contains(&child_rel) {
                continue;
            }
            let ignored = dir_ignored || is_path_ignored(&rules, &child_rel, false);
            out.push(UntrackedPath { path: child_rel, ignored });
        }
    }

    Ok(())
}

/// Reports whether a tracked path is excluded by `.gitignore` rules, either directly or through
/// an ignored ancestor directory. `cache` memoizes the accumulated rules per directory.
// Used by `ls-files` (`-i` combined with `-c` / `-m` / `-d`).
pub fn is_tracked_path_ignored(path: &str, cache: &mut HashMap<String, Vec<GitIgnoreRule>>) -> bool {
    let components: Vec<&str> = path.split('/').collect();
    for i in 0..components.len().saturating_sub(1) {
        let parent = components[..i].join("/");
        let dir = components[..=i].join("/");
        if is_path_ignored(&rules_for_dir(&parent, cache), &dir, true) {
            return true;
        }
    }
    let parent = components[..components.len().saturating_sub(1)].join("/");
    is_path_ignored(&rules_for_dir(&parent, cache), path, false)
}

fn rules_for_dir(dir_rel: &str, cache: &mut HashMap<String, Vec<GitIgnoreRule>>) -> Vec<GitIgnoreRule> {
    if let Some(rules) = cache.get(dir_rel) {
        return rules.clone();
    }
    let mut rules = if dir_rel.is_empty() {
        Vec::new()
    } else {
        let parent = dir_rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        rules_for_dir(parent, cache)
    };
    let dir_path = if dir_rel.is_empty() { Path::new(".") } else { Path::new(dir_rel) };
    rules.extend(read_gitignore_rules(dir_path, dir_rel));
    cache.insert(dir_rel.to_string(), rules.clone());
    rules
}

fn read_gitignore_rules(dir: &Path, base_rel: &str) -> Vec<GitIgnoreRule> {
    let file = dir.join(".gitignore");
    match fs::read_to_string(&file) {
        Ok(content) => content.lines().filter_map(|line| parse_gitignore_line(line, base_rel)).collect(),
        Err(_) => Vec::new(),
    }
}

/// Normalizes user-supplied pathspecs to forward-slash, root-relative form
/// (strips `./` prefixes and trailing slashes; `.` becomes the empty "match all" spec).
// Used by `ls-files` before calling `matches_ls_files_pathspec`.
pub fn normalize_pathspecs(specs: &[String]) -> Vec<String> {
    specs
        .iter()
        .map(|s| {
            let mut spec = s.replace('\\', "/");
            while let Some(rest) = spec.strip_prefix("./") {
                spec = rest.to_string();
            }
            if spec == "." {
                spec.clear();
            }
            spec.trim_end_matches('/').to_string()
        })
        .collect()
}

/// Checks a path against normalized pathspecs: an exact match, a directory prefix,
/// or a wildcard pattern (`*`, `?`). An empty spec list matches everything.
// Used by `ls-files` to restrict listed paths.
pub fn matches_ls_files_pathspec(path: &str, specs: &[String]) -> bool {
    if specs.is_empty() {
        return true;
    }
    let path = path.trim_end_matches('/');
    specs.iter().any(|spec| {
        spec.is_empty()
            || path == spec
            || path.starts_with(&format!("{}/", spec))
            || pathspec_wildcard_match(spec.as_bytes(), path.as_bytes())
    })
}

/// Matches git pathspec wildcards, where (unlike `.gitignore` globs) `*` also matches `/`,
/// so `*.rs` selects `src/main.rs`.
// Used by `ls-files` via `matches_ls_files_pathspec`.
pub fn pathspec_wildcard_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            backtrack = Some((p, t));
            p += 1;
        } else if let Some((star_p, star_t)) = backtrack {
            p = star_p + 1;
            t = star_t + 1;
            backtrack = Some((star_p, star_t + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}
