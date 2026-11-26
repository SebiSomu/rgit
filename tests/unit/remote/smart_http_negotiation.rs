//! Unit tests for `src/remote/smart_http/negotiation.rs`, compiled into that module via
//! `#[path]`. The negotiator runs on an in-memory commit graph; the end-to-end rounds against
//! real git are in `tests/cli/smart_http_fetch.rs`.

use super::*;
use std::collections::HashMap;

/// A commit graph: name -> (parents, date).
fn graph(edges: &[(&str, &[&str], i64)]) -> HashMap<String, (Vec<String>, i64)> {
    edges.iter().map(|(name, parents, date)| (name.to_string(), (parents.iter().map(|p| p.to_string()).collect(), *date))).collect()
}

/// Every `have` the negotiator offers, acknowledging the ones in `server_has` as soon as
/// they are offered (as the server would, one round at a time).
fn offered(commits: &HashMap<String, (Vec<String>, i64)>, tips: &[&str], server_has: &[&str]) -> Vec<String> {
    let tips: Vec<String> = tips.iter().map(|t| t.to_string()).collect();
    let mut negotiator = Negotiator::new(|oid: &str| commits.get(oid).cloned(), &tips);
    let mut out = Vec::new();
    while let Some(have) = negotiator.next_have() {
        if server_has.contains(&have.as_str()) {
            negotiator.ack(&have);
        }
        out.push(have);
    }
    out
}

#[test]
fn offers_commits_newest_first_and_each_once() {
    // a <- b <- c (tip), and a <- d (another tip): a is reached twice but offered once.
    let commits = graph(&[("a", &[], 1), ("b", &["a"], 2), ("c", &["b"], 4), ("d", &["a"], 3)]);
    assert_eq!(offered(&commits, &["c", "d"], &[]), ["c", "d", "b", "a"]);
}

#[test]
fn stops_below_an_acknowledged_commit() {
    // Local work c, d on top of b, which the server has: a (behind b) is never offered.
    let commits = graph(&[("a", &[], 1), ("b", &["a"], 2), ("c", &["b"], 3), ("d", &["c"], 4)]);
    assert_eq!(offered(&commits, &["d"], &["b"]), ["d", "c", "b"]);
}

#[test]
fn keeps_walking_other_lines_of_history() {
    // Two branches; the server has m1 (main) but nothing of the side branch, whose root s1
    // is older than m1. The side branch is still offered down to its root.
    let commits = graph(&[("m1", &[], 5), ("m2", &["m1"], 6), ("s1", &[], 1), ("s2", &["s1"], 2)]);
    assert_eq!(offered(&commits, &["m2", "s2"], &["m1"]), ["m2", "m1", "s2", "s1"]);
}

#[test]
fn an_acknowledged_commit_hides_what_it_reaches_on_every_path() {
    // x merges b and c, both on top of a; the server has the merge's parent b and c's
    // parent a. Once b is acknowledged, a (below b) is never sent, though c also reaches it.
    let commits = graph(&[("a", &[], 1), ("b", &["a"], 3), ("c", &["a"], 2), ("x", &["b", "c"], 4)]);
    assert_eq!(offered(&commits, &["x"], &["b", "a"]), ["x", "b", "c"]);
}

#[test]
fn skips_tips_that_are_not_local_commits() {
    let commits = graph(&[("a", &[], 1)]);
    assert_eq!(offered(&commits, &["missing", "a"], &[]), ["a"]);
}

#[test]
fn progress_waits_for_the_end_of_a_line() {
    // GitHub's progress arrives in chunks that split lines anywhere.
    let mut progress = RemoteProgress::default();
    assert_eq!(progress.feed(b"Counting objects: 100%"), b"");
    assert_eq!(progress.feed(b" (202/202), done.\nCompressing: 50%\r"), b"remote: Counting objects: 100% (202/202), done.\nremote: Compressing: 50%\r");
    assert_eq!(progress.feed(b"Total 4034 (from 3)"), b"");
    assert_eq!(progress.take_rest(), b"remote: Total 4034 (from 3)\n");
    assert_eq!(progress.take_rest(), b"");
}
