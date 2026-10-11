use std::collections::BTreeSet;
use std::fs;
use anyhow::Context;
use crate::helpers::clean::collect_clean_candidates;
use crate::index::read_index;
use crate::objects::{CleanMode, GitIgnoreRule};
use std::path::Path;

/// Removes files and directories from the working tree that are not tracked by the
/// index, similar to `git clean`. Requires either `dry_run` or `force`, matching
/// git's `clean.requireForce` default safety behavior.
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
