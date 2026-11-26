//! Reports for fetch, pull, push and clone.

use crate::ui::{
    {danger, detail, has, next, note, plural, positionals, short},
    narrate::{commits_between, commits_not_in, is_ancestor, take_snapshot},
    report::upstream_of,
    report::merge::report_merge,
    types::RepoSnapshot,
};
use std::collections::BTreeSet;

fn remote_changes(b: &RepoSnapshot, a: &RepoSnapshot) -> Vec<(String, Option<String>, Option<String>)> {
    let names: BTreeSet<&String> = b.refs.keys().chain(a.refs.keys()).filter(|n| n.starts_with("refs/remotes/") && !n.ends_with("/HEAD")).collect();
    names.into_iter().filter_map(|n| {
        let (x, y) = (b.refs.get(n).cloned(), a.refs.get(n).cloned());
        (x != y).then(|| (n.trim_start_matches("refs/remotes/").to_string(), x, y))
    }).collect()
}

fn describe_fetched(changes: &[(String, Option<String>, Option<String>)]) -> String {
    changes.iter().take(5).map(|(name, old, new)| match (old, new) {
        (Some(o), Some(n)) if is_ancestor(o, n) => format!("{} +{}", name, plural(commits_between(Some(o), n), "commit", "commits")),
        (Some(o), Some(n)) => format!("{} rewritten ({} -> {})", name, short(o), short(n)),
        (None, Some(_)) => format!("new branch {}", name),
        (Some(_), None) => format!("{} deleted on the remote", name),
        _ => String::new(),
    }).collect::<Vec<_>>().join(", ")
}

fn behind_line(a: &RepoSnapshot) {
    let (Some(branch), Some(head)) = (&a.head_branch, &a.head) else { return };
    let Some(up) = upstream_of(branch) else { return };
    let Some(up_hash) = a.refs.get(&up) else { return };
    let behind = commits_between(Some(head), up_hash);
    let ahead = commits_between(Some(up_hash), head);
    let short_up = crate::references::for_each_ref::engine::upstream::shorten_unambiguous_ref(&up);
    if behind > 0 {
        note(&format!("'{}' is now {} behind {}{}.", branch, plural(behind, "commit", "commits"), short_up, if ahead > 0 { format!(" and {} ahead (diverged)", ahead) } else { String::new() }));
        next(&format!("`rgit pull` (or `rgit merge {}`) to catch up.", short_up));
    }
}

pub(super) fn report_fetch(b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let changes = remote_changes(b, a);
    if changes.is_empty() {
        if ok == Some(true) {
            note("Already up to date: nothing new on the remote.");
        }
        return;
    }
    note(&format!("Fetched: {}.", describe_fetched(&changes)));
    detail("Fetching only updates remote-tracking branches (origin/...); your own branches and files are untouched.");
    behind_line(a);
}

pub(super) fn report_pull(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let changes = remote_changes(b, a);
    if changes.is_empty() && a.head == b.head {
        if ok == Some(true) {
            let up = a.head_branch.as_ref().and_then(|br| upstream_of(br)).map(|u| crate::references::for_each_ref::engine::upstream::shorten_unambiguous_ref(&u)).unwrap_or_else(|| "the remote".into());
            note(&format!("Already up to date: {} has nothing new for you.", up));
        }
        return;
    }
    if !changes.is_empty() {
        note(&format!("Fetched: {}.", describe_fetched(&changes)));
    }
    report_merge(args, b, a, ok, true);
}

pub(super) fn report_push(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let changes = remote_changes(b, a);
    let Some(branch) = &a.head_branch else { return };
    if changes.is_empty() {
        if ok == Some(true) {
            if let Some(head) = &a.head {
                let up = a.refs.iter().find(|(n, h)| n.starts_with("refs/remotes/") && n.ends_with(&format!("/{}", branch)) && *h == head);
                if let Some((name, h)) = up {
                    note(&format!("Nothing to push: {} is already at {}.", name.trim_start_matches("refs/remotes/"), short(h)));
                }
            }
        }
    } else {
        for (name, old, new) in &changes {
            match (old, new) {
                (Some(o), Some(n)) if !is_ancestor(o, n) => {
                    danger(&format!("Force-pushed {}: {} -> {}; the remote lost {} it had.", name, short(o), short(n), plural(commits_between(Some(n), o), "commit", "commits")));
                }
                (Some(o), Some(n)) => {
                    let sent = commits_between(Some(o), n);
                    note(&format!("Sent {} to {} ({}..{}).", plural(sent, "commit", "commits"), name, short(o), short(n)));
                }
                (None, Some(n)) => {
                    // A new branch: only what the remote's other branches didn't already have.
                    let remote = name.split('/').next().unwrap_or_default();
                    let prefix = format!("refs/remotes/{}/", remote);
                    let known: Vec<&str> = b.refs.iter().filter(|(r, _)| r.starts_with(&prefix)).map(|(_, h)| h.as_str()).collect();
                    match commits_not_in(n, &known) {
                        0 => note(&format!("Created {} at {}; the remote already had its commits.", name, short(n))),
                        sent => note(&format!("Sent {} to new branch {} ({}).", plural(sent, "commit", "commits"), name, short(n))),
                    }
                }
                (Some(o), None) => note(&format!("Deleted {} on the remote (was {}).", name, short(o))),
                _ => {}
            }
        }
    }
    if upstream_of(branch).is_none() && !has(args, &["-u", "--set-upstream"]) && ok == Some(true) {
        note(&format!("'{}' has no upstream yet, so plain `rgit pull`/`rgit push` don't know where to go.", branch));
        next("`rgit push -u` sets it (once).");
    }
    let _ = args;
}

// --- rebase / cherry-pick / revert -------------------------------------------------------

pub(in crate::ui) fn report_clone(args: &[String]) {
    let pos = positionals(args, &["-b", "--branch", "-o", "--origin"]);
    let Some(url) = pos.first() else { return };
    let dir = pos.get(1).cloned().unwrap_or_else(|| url.trim_end_matches('/').rsplit(['/', '\\', ':']).next().unwrap_or("").trim_end_matches(".git").to_string());
    if std::env::set_current_dir(&dir).is_err() {
        return;
    }
    let s = take_snapshot(false);
    let commits = s.head.as_deref().map(|h| commits_between(None, h)).unwrap_or(0);
    let branch = s.head_branch.clone().unwrap_or_else(|| "HEAD".into());
    note(&format!("Cloned {} into '{}'; checked out '{}' ({}).", plural(commits, "commit", "commits"), dir, branch, plural(s.index.len(), "file", "files")));
    next(&format!("`cd {}` to start working.", dir));
}
