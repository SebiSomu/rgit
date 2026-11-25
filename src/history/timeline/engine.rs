//! Helpers of `rgit timeline`: which branch each commit counts on, the merged branch of a merge
//! commit, and the events of the branches' history.

use std::collections::{BTreeMap, HashSet};

use anyhow::Result;

use crate::core::object_store::{flatten_tree, tree_hash_of_commit};
use crate::history::timeline::types::Event;

/// The default branch among `branches`: `init.defaultBranch`, else `main`, else `master`.
// Used by `branch_of`.
fn default_branch(branches: &[String]) -> Option<String> {
    let configured = crate::core::config::get_config_value("init.defaultBranch").ok().flatten();
    [configured.as_deref(), Some("main"), Some("master")].into_iter().flatten().find(|b| branches.iter().any(|x| x == b)).map(str::to_string)
}

/// Assigns commits to branches: a commit on the default branch's own line (its first parents)
/// counts on it; any other on the branch with the fewest commits that has it (the most
/// specific one), so the commits of a merged branch still count on that branch while it
/// exists; failing that, on the default branch (merged from a branch since deleted).
// Used by `collect_events`.
fn branch_of(branches: &[(String, String)]) -> Result<impl Fn(&str) -> Option<String>> {
    let names: Vec<String> = branches.iter().map(|(n, _)| n.clone()).collect();
    let default = default_branch(&names);
    let mut sets: Vec<(String, HashSet<String>)> = Vec::new();
    let mut fallback: Option<(String, HashSet<String>)> = None;
    for (name, tip) in branches {
        if Some(name) == default.as_ref() {
            let mut line = HashSet::new();
            let mut current = Some(tip.clone());
            while let Some(hash) = current {
                current = crate::core::commit::parse_commit_object(&hash)?.parents.first().cloned();
                line.insert(hash);
            }
            sets.push((name.clone(), line));
            fallback = Some((name.clone(), crate::core::commit::collect_ancestors(tip)?));
        } else {
            sets.push((name.clone(), crate::core::commit::collect_ancestors(tip)?));
        }
    }
    // The default branch's own line first, then the smallest histories.
    sets.sort_by(|a, b| (Some(&b.0) == default.as_ref()).cmp(&(Some(&a.0) == default.as_ref())).then(a.1.len().cmp(&b.1.len())).then(a.0.cmp(&b.0)));
    sets.extend(fallback);
    Ok(move |commit: &str| sets.iter().find(|(_, set)| set.contains(commit)).map(|(n, _)| n.clone()))
}

/// The branch a merge commit merged, from its message (`Merge branch 'x'`, `Merge branch 'x'
/// into y`, `Merge remote-tracking branch 'origin/x'`).
// Used by `collect_events`.
fn merged_name(subject: &str) -> String {
    let quoted = subject.split('\'').nth(1).filter(|_| subject.matches('\'').count() >= 2);
    quoted.map(str::to_string).unwrap_or_else(|| "a branch".to_string())
}

/// `+0200` as seconds east of UTC.
// Used by `collect_events`.
fn offset(tz: &str) -> i64 {
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let digits = tz.trim_start_matches(['+', '-']);
    let h: i64 = digits.get(..2).and_then(|h| h.parse().ok()).unwrap_or(0);
    let m: i64 = digits.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
    sign * (h * 3600 + m * 60)
}

/// The commits of every local branch (or only HEAD with `current`), newest first, authored at
/// or after `since`, as timeline events dated in the author's local time.
// Used by the `timeline` command.
pub fn collect_events(current: bool, since: Option<i64>) -> Result<Vec<Event>> {
    let mut tips: Vec<(String, String)> = Vec::new();
    if current {
        if let Some(head) = crate::core::refs::resolve_head_commit()? {
            let name = match crate::core::refs::resolve_head()? {
                crate::core::refs::HeadState::Branch(b) => b,
                crate::core::refs::HeadState::Detached(_) => "HEAD".to_string(),
            };
            tips.push((name, head));
        }
    } else {
        for name in crate::core::refs::list_branches()? {
            if let Some(tip) = crate::core::refs::read_ref(&format!("refs/heads/{}", name))? {
                tips.push((name, tip));
            }
        }
    }
    if tips.is_empty() {
        return Ok(Vec::new());
    }
    let label_branches = tips.len() > 1;
    let branch = branch_of(&tips)?;
    let opts = crate::history::rev_list::engine::parse_revision_args(&tips.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>())?;
    let (walked, _) = crate::history::rev_list::engine::select_rev_list_commits(&opts)?;
    let names = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let mut out = Vec::new();
    for w in walked {
        let c = &w.commit;
        if since.is_some_and(|s| c.author.timestamp < s) {
            continue;
        }
        let local = c.author.timestamp + offset(&c.author.tz);
        let day_number = local.div_euclid(86_400);
        let subject = c.message.lines().next().unwrap_or("").to_string();
        let (mut added, mut removed) = (0, 0);
        let merged = if c.parents.len() > 1 {
            Some(merged_name(&subject))
        } else {
            let mut old: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
            if let Some(p) = c.parents.first() {
                flatten_tree(&tree_hash_of_commit(p)?, "", &mut old)?;
            }
            let mut new = BTreeMap::new();
            flatten_tree(&c.tree, "", &mut new)?;
            let mut paths: Vec<&String> = old.keys().chain(new.keys()).filter(|p| old.get(*p) != new.get(*p)).collect();
            paths.sort();
            paths.dedup();
            for p in paths {
                let (a, r) = crate::history::hotspots::engine::line_changes(old.get(p).map(|e| e.0), new.get(p).map(|e| e.0))?;
                added += a;
                removed += r;
            }
            None
        };
        out.push(Event {
            subject,
            author: c.author.name.clone(),
            day: crate::history::story::engine::day(local),
            weekday: names[day_number.rem_euclid(7) as usize],
            day_number,
            branch: if label_branches { branch(&c.hash) } else { None },
            merged,
            added,
            removed,
        });
    }
    // Newest local day first; within a day, newest first (the walk's order).
    out.sort_by_key(|e| std::cmp::Reverse(e.day_number));
    Ok(out)
}
