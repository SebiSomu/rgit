use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::helpers::{is_path_ignored, normalize_path, parse_gitignore_line};
use crate::index::read_index;
use crate::objects::GitIgnoreRule;

enum CleanMode {
    Normal,
    IncludeIgnored,
    IgnoredOnly,
}

impl CleanMode {
    fn wants(&self, ignored: bool) -> bool {
        match self {
            CleanMode::Normal => !ignored,
            CleanMode::IncludeIgnored => true,
            CleanMode::IgnoredOnly => ignored,
        }
    }
}

/// Removes files and directories from the working tree that are not tracked by the
/// index, similar to `git clean`. Requires either `dry_run` or `force`, matching
/// git's `clean.requireForce` default safety behavior.
// Used by the `clean` command.
pub fn clean(dry_run: bool, force: bool, dirs: bool, ignored: bool, only_ignored: bool) -> anyhow::Result<()> {
    if ignored && only_ignored {
        anyhow::bail!("fatal: -x and -X cannot be used together");
    }

    if !dry_run && !force {
        anyhow::bail!(
            "fatal: clean.requireForce defaults to true and neither -n nor -f given; refusing to clean"
        );
    }

    let mode = if only_ignored {
        CleanMode::IgnoredOnly
    } else if ignored {
        CleanMode::IncludeIgnored
    } else {
        CleanMode::Normal
    };

    let index_entries = read_index().unwrap_or_default();
    let tracked_paths: BTreeSet<String> = index_entries.iter().map(|e| e.path.clone()).collect();

    // Every ancestor directory of every tracked path, so we can tell whether an
    // untracked directory has tracked content underneath it (and must therefore
    // be descended into rather than removed wholesale).
    let mut tracked_dirs: BTreeSet<String> = BTreeSet::new();
    for path in &tracked_paths {
        let mut components: Vec<&str> = path.split('/').collect();
        components.pop();
        let mut prefix = String::new();
        for comp in components {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(comp);
            tracked_dirs.insert(prefix.clone());
        }
    }

    let mut files_out: Vec<String> = Vec::new();
    let mut dirs_out: Vec<String> = Vec::new();
    let mut rules: Vec<GitIgnoreRule> = Vec::new();

    collect_clean_candidates(
        Path::new("."),
        &mut rules,
        &tracked_paths,
        &tracked_dirs,
        &mode,
        dirs,
        &mut files_out,
        &mut dirs_out,
    )?;

    files_out.sort();
    dirs_out.sort();

    if files_out.is_empty() && dirs_out.is_empty() {
        println!("nothing to clean, working tree already clean");
        return Ok(());
    }

    let verb = if dry_run { "Would remove" } else { "Removing" };

    for dir in &dirs_out {
        if !dry_run {
            fs::remove_dir_all(dir).with_context(|| format!("Failed to remove directory {}", dir))?;
        }
        println!("{} {}/", verb, dir);
    }
    for file in &files_out {
        if !dry_run {
            fs::remove_file(file).with_context(|| format!("Failed to remove {}", file))?;
        }
        println!("{} {}", verb, file);
    }

    Ok(())
}

/// Recursively walks the working tree (skipping `.git`) collecting untracked files
/// and, when `remove_dirs` is set, whole untracked directories, according to `mode`.
/// Mirrors the `.gitignore`-aware traversal used by `collect_files`, but additionally
/// distinguishes tracked vs. untracked content and never descends into a directory
/// it is about to remove wholesale.
// Used by `clean`.
fn collect_clean_candidates(path: &Path, rules: &mut Vec<GitIgnoreRule>, tracked_paths: &BTreeSet<String>, tracked_dirs: &BTreeSet<String>, mode: &CleanMode, remove_dirs: bool, files_out: &mut Vec<String>, dirs_out: &mut Vec<String>) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to stat {}", path.display()))?;
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

        // A directory with nothing tracked underneath it can be removed wholesale
        // (when -d is given) instead of being walked entry-by-entry.
        if remove_dirs && !has_tracked_content && !rel_path.is_empty() {
            if mode.wants(ignored) {
                dirs_out.push(rel_path);
                return Ok(());
            }
            if ignored {
                // Ignored directory that the current mode doesn't want: leave it
                // alone entirely, same as git's default refusal to descend into
                // ignored directories.
                return Ok(());
            }
        }

        // Otherwise recurse: either this directory holds tracked content (so we
        // must look for untracked files within it individually), -d wasn't given,
        // or it's ignored but the mode wants us to look inside for matches.
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
