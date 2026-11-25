pub mod cli;
pub mod engine;
pub mod types;

use std::collections::BTreeMap;

use crate::history::stats::engine::{bar, collect_commits, languages, sparkline};
use crate::history::stats::types::AuthorStat;
use crate::history::story::engine::day;

/// How many rows the author, language and biggest-commit tables show.
const TOP: usize = 10;
const BIGGEST: usize = 5;

/// `rgit stats [<path>...] [--since <date>] [--weeks <N>]`: numbers on the current branch's
/// non-merge commits: totals, weekly activity as a sparkline (ending at the newest commit, so
/// it reads the same any day), commits per weekday (author's local time), authors with their
/// lines, lines per language at HEAD, and the biggest commits (dated in the author's time).
pub fn stats(paths: &[String], since: Option<i64>, weeks: usize) -> anyhow::Result<()> {
    let Some(head) = crate::core::refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits yet");
    };
    let specs = crate::workspace::ls_files::engine::normalize_pathspecs(paths);
    let commits = collect_commits(&head, &specs, since)?;
    if commits.is_empty() {
        println!("No commits{}.", if since.is_some() { " in that time" } else { " change those paths" });
        return Ok(());
    }
    let mut authors: BTreeMap<&str, AuthorStat> = BTreeMap::new();
    for c in &commits {
        let a = authors.entry(&c.author).or_insert(AuthorStat { first: c.timestamp, last: c.timestamp, ..Default::default() });
        a.commits += 1;
        a.added += c.added;
        a.removed += c.removed;
        a.first = a.first.min(c.timestamp);
        a.last = a.last.max(c.timestamp);
    }
    let (added, removed): (usize, usize) = commits.iter().fold((0, 0), |(a, r), c| (a + c.added, r + c.removed));
    let newest = commits.iter().map(|c| c.timestamp).max().unwrap_or(0);
    let oldest = commits.iter().map(|c| c.timestamp).min().unwrap_or(0);
    let langs = languages(&head, &specs)?;
    let (lines_now, files_now): (usize, usize) = langs.values().fold((0, 0), |(l, f), v| (l + v.0, f + v.1));
    let n = commits.len();

    println!("{} commits by {} author{}, {} to {}", n, authors.len(), if authors.len() == 1 { "" } else { "s" }, day(oldest), day(newest));
    println!("{} lines in {} files today; +{} -{} lines over all commits ({} changed per commit on average)", lines_now, files_now, added, removed, (added + removed) / n);

    // Weekly activity, the last week being the newest commit's.
    let week = 7 * 86_400;
    let mut per_week = vec![0usize; weeks];
    for c in &commits {
        let back = ((newest - c.timestamp) / week) as usize;
        if back < weeks {
            per_week[weeks - 1 - back] += 1;
        }
    }
    let busiest = per_week.iter().copied().max().unwrap_or(0);
    println!();
    println!("Activity, last {} weeks to {} (busiest week: {} commit{})", weeks, day(newest), busiest, if busiest == 1 { "" } else { "s" });
    println!("  |{}|", sparkline(&per_week));

    // Weekday, in the author's local time (1970-01-01 was a Thursday).
    let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let mut per_day = [0usize; 7];
    for c in &commits {
        per_day[((c.timestamp + c.offset).div_euclid(86_400) + 3).rem_euclid(7) as usize] += 1;
    }
    let max_day = per_day.iter().copied().max().unwrap_or(0);
    println!();
    println!("By weekday");
    for (name, count) in names.iter().zip(per_day) {
        println!("  {}  {:>5}  {}", name, count, bar(count, max_day, 30));
    }

    let mut ranked: Vec<(&str, &AuthorStat)> = authors.iter().map(|(k, v)| (*k, v)).collect();
    ranked.sort_by(|a, b| b.1.commits.cmp(&a.1.commits).then(a.0.cmp(b.0)));
    let width = ranked.iter().take(TOP).map(|(a, _)| a.chars().count()).max().unwrap_or(0).max(6);
    println!();
    println!("Authors");
    for (name, a) in ranked.iter().take(TOP) {
        println!(
            "  {:<w$}  {:>5} commit{}  {:>3}%  {:<15}  {} to {}",
            name,
            a.commits,
            if a.commits == 1 { " " } else { "s" },
            a.commits * 100 / n,
            format!("+{} -{}", a.added, a.removed),
            day(a.first),
            day(a.last),
            w = width
        );
    }
    if ranked.len() > TOP {
        println!("  and {} more", ranked.len() - TOP);
    }

    if !langs.is_empty() {
        let mut by_lines: Vec<(&String, &(usize, usize))> = langs.iter().collect();
        by_lines.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
        let width = by_lines.iter().take(TOP).map(|(l, _)| l.chars().count()).max().unwrap_or(0);
        let max_lines = by_lines.first().map(|l| l.1.0).unwrap_or(0);
        println!();
        println!("Languages (lines today)");
        for (lang, (lines, files)) in by_lines.iter().take(TOP) {
            let share = (lines * 100).checked_div(lines_now).unwrap_or(0);
            println!("  {:<w$}  {:>7} lines  {:>3}%  {:>4} file{}  {}", lang, lines, share, files, if *files == 1 { " " } else { "s" }, bar(*lines, max_lines, 20), w = width);
        }
        if by_lines.len() > TOP {
            println!("  and {} more", by_lines.len() - TOP);
        }
    }

    let mut biggest: Vec<_> = commits.iter().collect();
    biggest.sort_by(|a, b| (b.added + b.removed).cmp(&(a.added + a.removed)).then(b.timestamp.cmp(&a.timestamp)));
    println!();
    println!("Biggest commits");
    for c in biggest.iter().take(BIGGEST) {
        println!("  {}  {}  {:<13}  {:>3} file{}  {} ({})", &c.hash[..7], day(c.timestamp + c.offset), format!("+{} -{}", c.added, c.removed), c.files, if c.files == 1 { " " } else { "s" }, c.subject, c.author);
    }
    Ok(())
}
