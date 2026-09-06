use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Context;
use crate::objects::GitIgnoreRule;

/// Matches a string against a glob pattern supporting `*`, `?`, and `**`.
// Used by `.gitignore` pattern matching in `helpers.rs`.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pat_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    glob_match_slice(&pat_chars, &text_chars)
}

fn glob_match_slice(pat: &[char], text: &[char]) -> bool {
    if pat.is_empty() {
        return text.is_empty();
    }

    if pat.starts_with(&['*', '*']) {
        let mut rest_pat = &pat[2..];
        if rest_pat.starts_with(&['/']) {
            rest_pat = &rest_pat[1..];
        }
        for i in 0..=text.len() {
            if glob_match_slice(rest_pat, &text[i..]) {
                return true;
            }
        }
        return false;
    }

    if pat[0] == '*' {
        let rest_pat = &pat[1..];
        let mut i = 0;
        while i <= text.len() {
            if glob_match_slice(rest_pat, &text[i..]) {
                return true;
            }
            if i < text.len() && text[i] == '/' {
                break;
            }
            i += 1;
        }
        return false;
    }

    if text.is_empty() {
        return false;
    }

    if pat[0] == '?' {
        if text[0] != '/' {
            return glob_match_slice(&pat[1..], &text[1..]);
        } else {
            return false;
        }
    }

    if pat[0] == text[0] {
        return glob_match_slice(&pat[1..], &text[1..]);
    }

    false
}

/// Parses a single line from a `.gitignore` file into a `GitIgnoreRule`.
// Used by `.gitignore` loading in `collect_files`.
pub fn parse_gitignore_line(line: &str, base_dir: &str) -> Option<GitIgnoreRule> {
    let mut trimmed = line.trim_end_matches(['\r', '\n']).trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let negated = if trimmed.starts_with('!') {
        trimmed = &trimmed[1..];
        true
    } else {
        false
    };

    let dir_only = if trimmed.ends_with('/') {
        trimmed = &trimmed[..trimmed.len() - 1];
        true
    } else {
        false
    };

    let (pattern, has_slash) = if trimmed.starts_with('/') {
        (&trimmed[1..], true)
    } else if trimmed.contains('/') {
        (trimmed, true)
    } else {
        (trimmed, false)
    };

    Some(GitIgnoreRule {
        pattern: pattern.to_string(),
        base_dir: base_dir.to_string(),
        negated,
        dir_only,
        has_slash,
    })
}

/// Checks whether a given relative path matches any active `.gitignore` rules.
// Used by `collect_files` and `status` checks.
pub fn is_path_ignored(rules: &[GitIgnoreRule], rel_path: &str, is_dir: bool) -> bool {
    let mut ignored = false;

    for rule in rules {
        if rule.dir_only && !is_dir {
            continue;
        }

        let target_subpath = if rule.base_dir.is_empty() {
            rel_path
        } else {
            if rel_path == rule.base_dir {
                ""
            } else if rel_path.starts_with(&format!("{}/", rule.base_dir)) {
                &rel_path[rule.base_dir.len() + 1..]
            } else {
                continue;
            }
        };

        let matched = if rule.has_slash {
            glob_match(&rule.pattern, target_subpath)
        } else {
            let filename = Path::new(rel_path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if glob_match(&rule.pattern, &filename) {
                true
            } else {
                target_subpath
                    .split('/')
                    .any(|part| glob_match(&rule.pattern, part))
            }
        };

        if matched {
            ignored = !rule.negated;
        }
    }

    ignored
}

/// Recursively collects all files under the given path, respecting `.gitignore` rules
/// and ignoring the `.git/` folder.
// Used by index staging (`add`), status, and working tree safety checks.
pub fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let mut rules = Vec::new();
    collect_files_internal(path, &mut rules, out)
}

fn collect_files_internal(path: &Path, rules: &mut Vec<GitIgnoreRule>, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let metadata = fs::metadata(path).with_context(|| format!("path '{}' did not match any files", path.display()))?;
    let rel_path = normalize_path(path);

    if metadata.is_file() {
        if !rel_path.is_empty() && is_path_ignored(rules, &rel_path, false) {
            return Ok(());
        }
        out.push(path.to_path_buf());
        return Ok(());
    }

    if metadata.is_dir() {
        let name_str = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if name_str == ".git" {
            return Ok(());
        }

        if !rel_path.is_empty() && is_path_ignored(rules, &rel_path, true) {
            return Ok(());
        }

        let mut current_rules = rules.clone();
        let gitignore_file = path.join(".gitignore");
        if gitignore_file.exists() && gitignore_file.is_file() {
            if let Ok(content) = fs::read_to_string(&gitignore_file) {
                for line in content.lines() {
                    if let Some(rule) = parse_gitignore_line(line, &rel_path) {
                        current_rules.push(rule);
                    }
                }
            }
        }

        let mut dir_entries: Vec<_> = fs::read_dir(path)?.filter_map(anyhow::Result::ok).collect();
        dir_entries.sort_by_key(|e| e.file_name());

        for entry in dir_entries {
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if entry_name == ".git" {
                continue;
            }
            collect_files_internal(&entry.path(), &mut current_rules, out)?;
        }
    }

    Ok(())
}

/// Normalizes a file path into a relative, forward-slash separated path string.
// Used for mapping paths consistently across platform boundaries in the index and object trees.
pub fn normalize_path(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}