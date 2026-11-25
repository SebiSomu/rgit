pub mod cli;
pub mod engine;
pub mod types;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use crate::workspace::ignore::engine::{append_patterns, clean_pattern, existing_lines, gitignore_stageable, plan_pattern, update_index};

/// `rgit ignore <pattern>... [--global] [--keep-tracked] [-n]`: adds patterns to the top-level
/// `.gitignore` (or the global ignore file) and stops tracking the files they now ignore. A
/// pattern the file already has is skipped; the file is never reordered (order matters for `!`
/// rules), new patterns go at the end. Untracked files stay on disk; `.gitignore` is staged
/// with the removals unless it had unstaged edits of its own. Says when a pattern matches
/// nothing yet (a typo?).
pub fn ignore(raw: Vec<String>, global: bool, keep_tracked: bool, dry_run: bool) -> anyhow::Result<()> {
    let (label, path) = if global {
        match crate::workspace::gitignore::global_excludes_path() {
            Some(p) => (p.to_string_lossy().replace('\\', "/"), p),
            None => crate::ui::die_fatal("no global ignore file: HOME is not set and core.excludesFile is not configured"),
        }
    } else {
        (".gitignore".to_string(), PathBuf::from(".gitignore"))
    };
    let mut patterns: Vec<String> = Vec::new();
    for p in &raw {
        match clean_pattern(p) {
            Ok(clean) if !patterns.contains(&clean) => patterns.push(clean),
            Ok(_) => {}
            Err(why) => crate::ui::die_fatal(why),
        }
    }

    let existing = existing_lines(&path);
    let index = crate::core::index::read_index().unwrap_or_default();
    let tracked: Vec<String> = index.iter().filter(|e| e.stage == 0).map(|e| e.path.clone()).collect();
    let tracked_set: BTreeSet<String> = index.iter().map(|e| e.path.clone()).collect();
    let untracked = crate::workspace::state_files::collect_untracked_files(&tracked_set, true).unwrap_or_default();
    let plans: Vec<_> = patterns.iter().map(|p| plan_pattern(p, &existing, &tracked, &untracked)).collect();
    let added: Vec<&str> = plans.iter().filter(|p| !p.already).map(|p| p.pattern.as_str()).collect();
    let already: Vec<&str> = plans.iter().filter(|p| p.already).map(|p| p.pattern.as_str()).collect();
    let stage_gitignore = !global && !dry_run && !added.is_empty() && gitignore_stageable()?;

    if !dry_run && !added.is_empty() {
        append_patterns(&path, &added)?;
    }
    // Tracked files the patterns match and that the rules as a whole now ignore (a later `!`
    // rule can keep one); without writing the file (dry run), the patterns alone decide.
    let mut candidates: BTreeSet<String> = plans.iter().flat_map(|p| p.tracked.iter().cloned()).collect();
    if !dry_run {
        let mut cache = HashMap::new();
        candidates.retain(|p| crate::workspace::ls_files::engine::is_tracked_path_ignored(p, &mut cache));
    }
    let untrack: Vec<String> = if keep_tracked { Vec::new() } else { candidates.into_iter().filter(|p| p != ".gitignore").collect() };
    if !dry_run && (!untrack.is_empty() || stage_gitignore) {
        update_index(&untrack, stage_gitignore)?;
    }

    let would = |done: &str, planned: &str| if dry_run { planned.to_string() } else { done.to_string() };
    if !added.is_empty() {
        println!("{} {}: {}", would("Added to", "Would add to"), label, added.join(", "));
    }
    if !already.is_empty() {
        println!("Already in {}: {}", label, already.join(", "));
    }
    for plan in plans.iter().filter(|p| !p.already && p.tracked.is_empty() && p.untracked == 0 && !p.pattern.starts_with('!')) {
        println!("  note: '{}' matches no file yet (a typo?)", plan.pattern);
    }
    if !untrack.is_empty() {
        println!("{} {} that {} now ignored (kept on disk): {}", would("Stopped tracking", "Would stop tracking"), files(untrack.len()), if untrack.len() == 1 { "is" } else { "are" }, list(&untrack));
    } else if keep_tracked && plans.iter().any(|p| !p.tracked.is_empty()) {
        let kept: BTreeSet<&String> = plans.iter().flat_map(|p| p.tracked.iter()).collect();
        println!("Still tracked (--keep-tracked), so not ignored: {}", list(&kept.into_iter().cloned().collect::<Vec<_>>()));
    }
    if dry_run {
        println!("(dry run: nothing changed)");
    } else if stage_gitignore || !untrack.is_empty() {
        let staged = match (stage_gitignore, untrack.is_empty()) {
            (true, true) => "Staged .gitignore".to_string(),
            (true, false) => "Staged .gitignore and the removals".to_string(),
            (false, _) => "Staged the removals (.gitignore has other unstaged edits: `rgit add .gitignore` when ready)".to_string(),
        };
        println!("{}; `rgit commit` records {}.", staged, if stage_gitignore && untrack.is_empty() { "it" } else { "them" });
    } else if !global && !added.is_empty() {
        println!(".gitignore has other unstaged edits, so it isn't staged: `rgit add .gitignore` when ready.");
    }
    Ok(())
}

/// `1 file` / `3 files`.
// Used by `ignore`.
fn files(n: usize) -> String {
    format!("{} file{}", n, if n == 1 { "" } else { "s" })
}

/// Up to 6 paths, then `and N more`.
// Used by `ignore`.
fn list(paths: &[String]) -> String {
    let mut out = paths.iter().take(6).cloned().collect::<Vec<_>>().join(", ");
    if paths.len() > 6 {
        out.push_str(&format!(" and {} more", paths.len() - 6));
    }
    out
}
