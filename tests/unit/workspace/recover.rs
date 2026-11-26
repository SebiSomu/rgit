//! Unit tests for `src/workspace/recover/engine.rs`, compiled into that module via `#[path]`.
//! `lost_tips` runs on an in-memory commit graph; finding lost work in a real repository is
//! covered by the `recover` scenario in `tests/cli/scenarios/recover.rs`.

use super::*;

/// A commit graph: name -> parents.
fn graph(edges: &[(&str, &[&str])]) -> HashMap<String, CommitFacts> {
    edges
        .iter()
        .map(|(name, parents)| (name.to_string(), CommitFacts { parents: parents.iter().map(|p| p.to_string()).collect(), ..Default::default() }))
        .collect()
}

fn tips(commits: &HashMap<String, CommitFacts>, candidates: &[&str], kept: &[&str]) -> Vec<(String, usize)> {
    let candidates: Vec<String> = candidates.iter().map(|c| c.to_string()).collect();
    let kept: HashSet<String> = kept.iter().map(|k| k.to_string()).collect();
    let mut out = lost_tips(&candidates, &kept, &mut |h: &str| commits.get(h).cloned());
    out.sort();
    out
}

#[test]
fn a_lost_line_shows_once_as_its_tip_with_its_length() {
    // main is at b; c, d, e were reset away (all three are in the reflog, in any order).
    let commits = graph(&[("a", &[]), ("b", &["a"]), ("c", &["b"]), ("d", &["c"]), ("e", &["d"])]);
    assert_eq!(tips(&commits, &["c", "e", "d"], &["a", "b"]), [("e".to_string(), 3)]);
}

#[test]
fn separate_lost_lines_are_listed_separately() {
    let commits = graph(&[("a", &[]), ("x1", &["a"]), ("x2", &["x1"]), ("y1", &["a"])]);
    assert_eq!(tips(&commits, &["x1", "x2", "y1"], &["a"]), [("x2".to_string(), 2), ("y1".to_string(), 1)]);
}

#[test]
fn a_lost_merge_covers_both_lost_sides() {
    let commits = graph(&[("a", &[]), ("l", &["a"]), ("r", &["a"]), ("m", &["l", "r"])]);
    assert_eq!(tips(&commits, &["l", "r", "m"], &["a"]), [("m".to_string(), 3)]);
}

#[test]
fn dates_are_shown_in_the_commits_own_timezone() {
    assert_eq!(format_when(1_764_165_780, "+0000"), "2025-11-26 14:03");
    assert_eq!(format_when(1_764_165_780, "+0200"), "2025-11-26 16:03");
    assert_eq!(format_when(1_764_165_780, "-0530"), "2025-11-26 08:33");
}
