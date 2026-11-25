pub mod cli;
pub mod engine;
pub mod types;

use std::collections::BTreeMap;

use crate::history::story::engine::{day, is_directory, ownership, walk_history};
use crate::history::story::types::Event;

/// A rewrite: a commit that replaced at least half of a file of at least this many lines.
const REWRITE_MIN_LINES: usize = 5;

/// `rgit story <path> [-n <N> | --all]`: the history of a file or directory in plain words:
/// when and by whom it was created (under which name), renames, how many commits and authors
/// changed it and how many lines, the big rewrites, who changed it most, who wrote its lines
/// today (blame), the last change, and a timeline of the newest commits.
pub fn story(raw_path: &str, timeline: usize) -> anyhow::Result<()> {
    let path = raw_path.trim_end_matches('/').trim_start_matches("./").replace('\\', "/");
    let Some(head) = crate::core::refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits yet");
    };
    let directory = is_directory(&head, &path)?;
    let events = walk_history(&head, &path, directory)?;
    if events.is_empty() {
        crate::ui::die_fatal(format!("'{}' has no history in this branch", path));
    }
    let when = |e: &Event| format!("{} ({})", day(e.timestamp), crate::ui::narrate::relative_time(e.timestamp));
    let at = |e: &Event| format!("{} \"{}\"", &e.commit[..7], e.subject);
    let row = |label: &str, value: String| println!("{:<13}{}", label, value);
    let exists_now = directory || crate::history::blame::engine::blob_hash_at_path(&head, &path)?.is_some();

    println!("{}{}", path, if directory { "/ (directory)" } else if exists_now { "" } else { " (deleted)" });
    let first = events.last().expect("not empty");
    let original_name = events.iter().rev().find_map(|e| e.renamed.as_ref().map(|(old, _)| old.clone()));
    let created_name = match &original_name {
        Some(name) if !directory => format!(" as {}", name),
        _ => String::new(),
    };
    row(if directory { "First commit" } else { "Created" }, format!("{} by {} in {}{}", when(first), first.author, at(first), created_name));
    for e in events.iter().rev().filter(|e| e.renamed.is_some()) {
        let (old, new) = e.renamed.as_ref().unwrap();
        row("Renamed", format!("{} -> {} in {} ({}, {})", old, new, at(e), e.author, day(e.timestamp)));
    }

    let mut by_author: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &events {
        *by_author.entry(&e.author).or_default() += 1;
    }
    let (added, removed): (usize, usize) = events.iter().fold((0, 0), |(a, r), e| (a + e.added.unwrap_or(0), r + e.removed.unwrap_or(0)));
    row("Changed", format!("{}, by {}; +{} -{} lines", plural(events.len(), "commit", "commits"), plural(by_author.len(), "author", "authors"), added, removed));
    let mut ranked: Vec<(&str, usize)> = by_author.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    if ranked.len() > 1 {
        row("Most commits", top(&ranked.iter().map(|(a, n)| (a.to_string(), *n)).collect::<Vec<_>>(), |n| plural(n, "commit", "commits")));
    }

    if !directory {
        let rewrites: Vec<&Event> = events
            .iter()
            .filter(|e| !e.created && e.old_lines >= REWRITE_MIN_LINES && e.removed.is_some_and(|r| r * 2 >= e.old_lines))
            .collect();
        for e in rewrites.iter().take(3) {
            let percent = e.removed.unwrap_or(0) * 100 / e.old_lines.max(1);
            row("Rewrite", format!("{} replaced {}% of its lines in {} ({})", e.author, percent.min(100), at(e), day(e.timestamp)));
        }
    }

    if exists_now {
        let owners = ownership(&head, &path, directory)?;
        let total: usize = owners.values().sum();
        if total > 0 {
            let mut owned: Vec<(String, usize)> = owners.into_iter().collect();
            owned.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            row("Lines today", top(&owned, |n| format!("{}%", (n * 100 + total / 2) / total)));
        }
    } else {
        let gone = &events[0];
        row("Deleted", format!("{} by {} in {}", when(gone), gone.author, at(gone)));
    }
    let last = &events[0];
    if exists_now {
        row("Last change", format!("{} by {} in {}", when(last), last.author, at(last)));
    }

    println!();
    println!("Timeline (newest first{}):", if events.len() > timeline { format!(", {} of {}; --all for every one", timeline, events.len()) } else { String::new() });
    let width = events.iter().take(timeline).map(|e| e.author.chars().count()).max().unwrap_or(0);
    for e in events.iter().take(timeline) {
        let change = match (e.added, e.removed) {
            (Some(a), Some(r)) => format!("+{} -{}", a, r),
            _ => "binary".to_string(),
        };
        let mark = if e.created { "  (created)" } else if e.deleted { "  (deleted)" } else if e.renamed.is_some() { "  (renamed)" } else { "" };
        println!("  {}  {}  {:<width$}  {:<9}  {}{}", day(e.timestamp), &e.commit[..7], e.author, change, e.subject, mark, width = width);
    }
    Ok(())
}

/// `1 commit` / `3 commits`.
// Used by `story`.
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// The first three `(name, n)` as `name (shown n)`, then `and N others`.
// Used by `story`.
fn top(ranked: &[(String, usize)], show: impl Fn(usize) -> String) -> String {
    let mut out: Vec<String> = ranked.iter().take(3).map(|(name, n)| format!("{} ({})", name, show(*n))).collect();
    if ranked.len() > 3 {
        out.push(format!("and {} other{}", ranked.len() - 3, if ranked.len() == 4 { "" } else { "s" }));
    }
    out.join(", ")
}
