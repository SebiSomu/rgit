use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::helpers::{is_path_ignored, normalize_path, parse_gitignore_line};
use crate::objects::{CleanMode, GitIgnoreRule};

/// Recursively walks the working tree (skipping `.git`) collecting untracked files
/// and, when `remove_dirs` is set, whole untracked directories, according to `mode`.
/// Mirrors the `.gitignore`-aware traversal used by `collect_files`, but additionally
/// distinguishes tracked vs. untracked content and never descends into a directory
/// it is about to remove wholesale.
// Used by `clean`.
pub fn collect_clean_candidates(path: &Path, rules: &mut Vec<GitIgnoreRule>, tracked_paths: &BTreeSet<String>, tracked_dirs: &BTreeSet<String>, mode: &CleanMode, remove_dirs: bool, files_out: &mut Vec<String>, dirs_out: &mut Vec<String>,) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path).with_context(|| format!("Failed to stat {}", path.display()))?;
    let rel_path = normalize_path(path);

    if metadata.is_file() || metadata.file_type().is_symlink() {
        if tracked_paths.contains(&rel_path) {
            return Ok(());
        }
        let ignored = !rel_path.is_empty() && is_path_ignored(rules, &rel_path, false);
        if mode.wants(ignored) {
            files_out.push(rel_path);
        }
        return Ok(());
    }

    if metadata.is_dir() {
        let name_str = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if name_str == ".git" {
            return Ok(());
        }

        let ignored = !rel_path.is_empty() && is_path_ignored(rules, &rel_path, true);
        let has_tracked_content = tracked_dirs.contains(&rel_path);

        if remove_dirs && !has_tracked_content && !rel_path.is_empty() {
            if mode.wants(ignored) {
                dirs_out.push(rel_path);
                return Ok(());
            }
            if ignored {
                return Ok(());
            }
        }

        if ignored && !mode.wants(true) {
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
            collect_clean_candidates(
                &entry.path(),
                &mut current_rules,
                tracked_paths,
                tracked_dirs,
                mode,
                remove_dirs,
                files_out,
                dirs_out,
            )?;
        }
    }

    Ok(())
}
