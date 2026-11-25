pub mod cli;
pub mod engine;
pub mod types;

use std::collections::BTreeMap;

use crate::history::timeline::engine::collect_events;
use crate::history::timeline::types::Event;

/// Subjects shown per day before `+N more`.
const SUBJECTS: usize = 3;

/// `rgit timeline [-n <days>] [--since <date>] [--current]`: a day-by-day digest of the
/// repository, newest day first, two lines a day: how many commits, by whom, on which branches
/// (every local branch by default) and the lines changed; then the merges and the first
/// subjects. Days are the authors' local days; quiet stretches between shown days are noted.
pub fn timeline(days: usize, since: Option<i64>, current: bool) -> anyhow::Result<()> {
    let events = collect_events(current, since)?;
    if events.is_empty() {
        println!("No commits{}.", if since.is_some() { " in that time" } else { " yet" });
        return Ok(());
    }
    let mut by_day: Vec<(i64, Vec<&Event>)> = Vec::new();
    for e in &events {
        match by_day.last_mut() {
            Some((d, list)) if *d == e.day_number => list.push(e),
            _ => by_day.push((e.day_number, vec![e])),
        }
    }
    let total_days = by_day.len();
    let mut previous: Option<i64> = None;
    for (day, list) in by_day.iter().take(days) {
        if let Some(prev) = previous
            && prev - day > 1
        {
            let gap = prev - day - 1;
            println!("                (quiet for {} day{})", gap, if gap == 1 { "" } else { "s" });
        }
        previous = Some(*day);
        let first = list[0];
        let commits: Vec<&&Event> = list.iter().filter(|e| e.merged.is_none()).collect();
        let merges: Vec<&&Event> = list.iter().filter(|e| e.merged.is_some()).collect();

        let mut head = format!("{} {}  ", first.weekday, first.day);
        let mut counts = Vec::new();
        if !commits.is_empty() {
            counts.push(format!("{} commit{}", commits.len(), if commits.len() == 1 { "" } else { "s" }));
        }
        if !merges.is_empty() {
            counts.push(format!("{} merge{}", merges.len(), if merges.len() == 1 { "" } else { "s" }));
        }
        head.push_str(&counts.join(", "));
        head.push_str(&format!(" by {}", ranked(list.iter().map(|e| e.author.as_str()))));
        if list.iter().any(|e| e.branch.is_some()) {
            head.push_str(&format!(" on {}", ranked(list.iter().filter_map(|e| e.branch.as_deref()))));
        }
        let (added, removed) = commits.iter().fold((0, 0), |(a, r), e| (a + e.added, r + e.removed));
        if !commits.is_empty() {
            head.push_str(&format!("  +{} -{}", added, removed));
        }
        println!("{}", head);

        let mut notes: Vec<String> = merges.iter().map(|m| format!("merged {}{}", m.merged.as_deref().unwrap_or("a branch"), m.branch.as_ref().map(|b| format!(" into {}", b)).unwrap_or_default())).collect();
        notes.extend(commits.iter().take(SUBJECTS).map(|e| e.subject.clone()));
        if commits.len() > SUBJECTS {
            notes.push(format!("+{} more", commits.len() - SUBJECTS));
        }
        println!("                {}", notes.join(" · "));
    }
    if total_days > days {
        println!("({} of {} days with commits; -n {} shows more)", days, total_days, total_days);
    }
    Ok(())
}

/// `Sebi (3), Ana`: names by how often they occur (most first, then by name), with the count
/// when it is more than one.
// Used by `timeline`.
fn ranked<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for n in names {
        *counts.entry(n).or_default() += 1;
    }
    let mut list: Vec<(&str, usize)> = counts.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    list.iter().map(|(n, c)| if *c > 1 { format!("{} ({})", n, c) } else { n.to_string() }).collect::<Vec<_>>().join(", ")
}
