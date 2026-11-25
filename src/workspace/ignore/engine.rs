//! Helpers of `rgit ignore`: pattern clean-up, what a pattern matches, appending to the ignore
//! file, and untracking / staging in the index.

use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::workspace::gitignore::{ignore_rule_matches, parse_gitignore_line};
use crate::workspace::ignore::types::PatternPlan;
use crate::workspace::types::GitIgnoreRule;

/// A pattern as it goes into the file: trimmed, with `\` path separators turned into `/`.
/// Refuses an empty pattern and one starting with `#` (that line would be a comment).
// Used by the `ignore` command.
pub fn clean_pattern(raw: &str) -> Result<String, String> {
    let pattern = raw.trim().replace('\\', "/");
    if pattern.is_empty() {
        return Err("an empty pattern ignores nothing".to_string());
    }
    if pattern.starts_with('#') {
        return Err(format!("'{}' would be a comment in .gitignore\nhint: write it as '\\{}' to match a name starting with #", pattern, pattern));
    }
    Ok(pattern)
}

/// The lines of an ignore file, trimmed (none when it doesn't exist).
// Used by the `ignore` command, to skip patterns the file already has.
pub fn existing_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path).map(|c| c.lines().map(|l| l.trim().to_string()).collect()).unwrap_or_default()
}

/// Whether `rule` matches `path` or one of its parent directories.
// Used by `plan_pattern`.
fn matches(rule: &GitIgnoreRule, path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    (1..parts.len()).any(|i| ignore_rule_matches(rule, &parts[..i].join("/"), true)) || ignore_rule_matches(rule, path, false)
}

/// What a pattern does: whether `existing` already has it, and the tracked paths and the
/// number of untracked paths it matches (none for a `!` pattern, which un-ignores).
// Used by the `ignore` command.
pub fn plan_pattern(pattern: &str, existing: &[String], tracked: &[String], untracked: &[String]) -> PatternPlan {
    let already = existing.iter().any(|l| l == pattern);
    let (mut matched_tracked, mut matched_untracked) = (Vec::new(), 0);
    if let Some(rule) = parse_gitignore_line(pattern, "")
        && !rule.negated
    {
        matched_tracked = tracked.iter().filter(|p| matches(&rule, p)).cloned().collect();
        matched_untracked = untracked.iter().filter(|p| matches(&rule, p)).count();
    }
    PatternPlan { pattern: pattern.to_string(), already, tracked: matched_tracked, untracked: matched_untracked }
}

/// Appends `patterns` to the ignore file, one per line, adding the missing final newline first.
// Used by the `ignore` command.
pub fn append_patterns(path: &Path, patterns: &[&str]) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let mut content = fs::read_to_string(path).unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    for p in patterns {
        content.push_str(p);
        content.push('\n');
    }
    fs::write(path, content)?;
    Ok(())
}

/// Whether `.gitignore` can be staged without staging anything else: it is new, or its staged
/// copy matches the file.
// Used by the `ignore` command, before editing the file.
pub fn gitignore_stageable() -> Result<bool> {
    let entries = crate::core::index::read_index().unwrap_or_default();
    let Some(entry) = entries.iter().find(|e| e.path == ".gitignore" && e.stage == 0) else {
        return Ok(!Path::new(".gitignore").exists());
    };
    let content = crate::core::eol::read_clean(".gitignore")?;
    Ok(crate::core::object_store::hash_content("blob", &content) == entry.hash)
}

/// Removes `untrack` from the index (the files stay on disk) and, when `stage_gitignore`,
/// stages `.gitignore` as it is now.
// Used by the `ignore` command.
pub fn update_index(untrack: &[String], stage_gitignore: bool) -> Result<()> {
    let mut entries = crate::core::index::read_index().unwrap_or_default();
    entries.retain(|e| !untrack.contains(&e.path));
    if stage_gitignore {
        let content = crate::core::eol::read_clean(".gitignore")?;
        let hash = crate::core::object_store::write_object("blob", &content)?;
        let mut raw = [0u8; 20];
        hex::decode_to_slice(&hash, &mut raw)?;
        let entry = crate::core::index::build_entry(".gitignore", raw, &fs::metadata(".gitignore")?);
        entries.retain(|e| e.path != ".gitignore");
        entries.push(entry);
        entries.sort_by(|a, b| a.path.cmp(&b.path));
    }
    crate::core::index::write_index(&mut entries)?;
    Ok(())
}
