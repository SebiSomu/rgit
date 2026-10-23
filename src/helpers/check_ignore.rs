use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use crate::helpers::{ignore_rule_matches, parse_gitignore_line};
use crate::objects::SourcedIgnoreRule;

/// Finds the `.gitignore` rule deciding a path's fate, mirroring git's lookup: leading
/// directories are checked first (outermost first), and the first one excluded by a
/// non-negated rule decides for everything beneath it; otherwise the last rule matching the
/// path itself is returned, which may be a negated (`!`) rule.
// Used by `check-ignore`.
pub fn find_ignore_match(path: &str, is_dir: bool, cache: &mut HashMap<String, Vec<SourcedIgnoreRule>>) -> Option<SourcedIgnoreRule> {
    let components: Vec<&str> = path.split('/').collect();

    for i in 0..components.len().saturating_sub(1) {
        let parent = components[..i].join("/");
        let dir = components[..=i].join("/");
        let rules = sourced_rules_for_dir(&parent, cache);
        if let Some(found) = last_matching_rule(&rules, &dir, true) {
            if !found.rule.negated {
                return Some(found);
            }
        }
    }

    let parent = components[..components.len().saturating_sub(1)].join("/");
    let rules = sourced_rules_for_dir(&parent, cache);
    last_matching_rule(&rules, path, is_dir)
}

fn last_matching_rule(rules: &[SourcedIgnoreRule], path: &str, is_dir: bool) -> Option<SourcedIgnoreRule> {
    rules.iter().rev().find(|r| ignore_rule_matches(&r.rule, path, is_dir)).cloned()
}

fn sourced_rules_for_dir(dir_rel: &str, cache: &mut HashMap<String, Vec<SourcedIgnoreRule>>) -> Vec<SourcedIgnoreRule> {
    if let Some(rules) = cache.get(dir_rel) {
        return rules.clone();
    }
    let mut rules = if dir_rel.is_empty() {
        Vec::new()
    } else {
        let parent = dir_rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        sourced_rules_for_dir(parent, cache)
    };

    let source = if dir_rel.is_empty() { ".gitignore".to_string() } else { format!("{}/.gitignore", dir_rel) };
    if let Ok(content) = fs::read_to_string(&source) {
        for (idx, line) in content.lines().enumerate() {
            if let Some(rule) = parse_gitignore_line(line, dir_rel) {
                rules.push(SourcedIgnoreRule {
                    rule,
                    source: source.clone(),
                    line_number: idx + 1,
                    pattern_text: line.trim().to_string(),
                });
            }
        }
    }

    cache.insert(dir_rel.to_string(), rules.clone());
    rules
}

/// Converts a user-supplied path into a forward-slash, repository-relative path and reports
/// whether it names a directory (trailing slash or an existing directory on disk).
/// Absolute paths inside the current directory are accepted; paths escaping it are rejected.
// Used by `check-ignore`.
pub fn normalize_check_ignore_path(arg: &str) -> anyhow::Result<(String, bool)> {
    let mut spec = arg.replace('\\', "/");

    if Path::new(arg).is_absolute() {
        let cwd = std::env::current_dir()?;
        let relative = Path::new(arg)
            .strip_prefix(&cwd)
            .map_err(|_| anyhow::anyhow!("fatal: {}: '{}' is outside repository at '{}'", arg, arg, cwd.display()))?;
        spec = relative.to_string_lossy().replace('\\', "/");
    }

    let trailing_slash = spec.ends_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in spec.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    let cwd = std::env::current_dir()?;
                    anyhow::bail!("fatal: {}: '{}' is outside repository at '{}'", arg, arg, cwd.display());
                }
            }
            other => parts.push(other),
        }
    }

    let normalized = parts.join("/");
    let is_dir = trailing_slash || (!normalized.is_empty() && Path::new(&normalized).is_dir());
    Ok((normalized, is_dir))
}

/// Reports whether a path is tracked, either as an index entry itself or as a directory
/// containing tracked entries (git skips both unless `--no-index` is given).
// Used by `check-ignore`.
pub fn is_tracked_or_contains_tracked(path: &str, tracked: &BTreeSet<String>) -> bool {
    if tracked.contains(path) {
        return true;
    }
    let prefix = format!("{}/", path);
    tracked.range(prefix.clone()..).next().is_some_and(|p| p.starts_with(&prefix))
}
