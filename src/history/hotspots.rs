pub mod cli;
pub mod engine;
pub mod types;

use crate::history::hotspots::engine::{COUPLING_MAX_FILES, collect};

/// Pairs need at least this many shared commits to count as changed together.
const COUPLING_MIN_SHARED: usize = 2;

/// `rgit hotspots [<path>...] [--since <date>] [-n <N>]`: the files that change most (commits
/// and lines), the ones touched by most authors, and the pairs that change together (coupling),
/// over the current branch's non-merge commits. Only files that still exist count, and a file
/// changed in a single commit is not listed.
pub fn hotspots(paths: &[String], since: Option<i64>, since_text: Option<&str>, limit: usize) -> anyhow::Result<()> {
    let Some(head) = crate::core::refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits yet");
    };
    let h = collect(&head, paths, since)?;
    if h.files.is_empty() {
        println!("No commits changed {}{}.", if paths.is_empty() { "any file that still exists".to_string() } else { paths.join(", ") }, since_text.map(|s| format!(" since {}", s)).unwrap_or_default());
        return Ok(());
    }
    println!(
        "Hotspots over {} commit{}{} ({} file{}):",
        h.commits,
        if h.commits == 1 { "" } else { "s" },
        since_text.map(|s| format!(" since {}", s)).unwrap_or_default(),
        h.files.len(),
        if h.files.len() == 1 { "" } else { "s" }
    );

    println!("\nMost changed");
    // A file changed once is not a hotspot.
    let mut changed: Vec<_> = h.files.iter().filter(|(_, s)| s.commits > 1).collect();
    if changed.is_empty() {
        println!("  no file changed more than once");
    }
    changed.sort_by(|a, b| b.1.commits.cmp(&a.1.commits).then((b.1.added + b.1.removed).cmp(&(a.1.added + a.1.removed))).then(a.0.cmp(b.0)));
    let churn: Vec<String> = changed.iter().take(limit).map(|(_, s)| format!("+{} -{}", s.added, s.removed)).collect();
    let churn_width = churn.iter().map(String::len).max().unwrap_or(0);
    for ((path, s), churn) in changed.iter().take(limit).zip(&churn) {
        println!("  {:>4} commit{}  {:<w$}  {}", s.commits, if s.commits == 1 { " " } else { "s" }, churn, path, w = churn_width);
    }

    let mut by_authors: Vec<_> = h.files.iter().filter(|(_, s)| s.authors.len() > 1).collect();
    if !by_authors.is_empty() {
        println!("\nMost authors");
        by_authors.sort_by(|a, b| b.1.authors.len().cmp(&a.1.authors.len()).then(b.1.commits.cmp(&a.1.commits)).then(a.0.cmp(b.0)));
        for (path, s) in by_authors.iter().take(limit) {
            let mut ranked: Vec<(&String, &usize)> = s.authors.iter().collect();
            ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let mut who: Vec<String> = ranked.iter().take(3).map(|(a, n)| format!("{} {}", a, n)).collect();
            if ranked.len() > 3 {
                who.push(format!("+{} more", ranked.len() - 3));
            }
            println!("  {:>4} authors  {}  ({})", s.authors.len(), path, who.join(", "));
        }
    }

    let mut pairs: Vec<_> = h.pairs.iter().filter(|(_, n)| **n >= COUPLING_MIN_SHARED).collect();
    if !pairs.is_empty() {
        let left_out = match h.bulk {
            0 => String::new(),
            n => format!(" (leaving out {} commit{} of more than {} files)", n, if n == 1 { "" } else { "s" }, COUPLING_MAX_FILES),
        };
        println!("\nChanged together{}", left_out);
        let share = |((a, b), n): &(&(String, String), &usize)| -> usize { **n * 100 / h.files[a].commits.min(h.files[b].commits).max(1) };
        pairs.sort_by(|x, y| y.1.cmp(x.1).then(share(y).cmp(&share(x))).then(x.0.cmp(y.0)));
        for pair in pairs.iter().take(limit) {
            let ((a, b), n) = pair;
            let (fewer, _) = if h.files[a].commits <= h.files[b].commits { (a, b) } else { (b, a) };
            println!("  {:>4} commits  {} <-> {}  ({}% of {}'s changes)", n, a, b, share(pair), fewer);
        }
    }
    Ok(())
}
