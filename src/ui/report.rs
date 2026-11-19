//! Narration after a command: the `report` dispatcher, per-command reports and the shared summaries they use.

pub mod merge;
pub mod refs;
pub mod remote;
pub mod workspace;

use crate::ui::{
    {detail, level, list, next, plural, short, teach, warn},
    narrate::{commits_between, is_ancestor, scan_risks, tree_changes},
    report::merge::{report_conflicts, report_merge, report_pick, report_rebase},
    report::refs::{report_branch, report_log, report_tag},
    report::remote::{report_fetch, report_pull, report_push},
    report::workspace::{report_add, report_checkout, report_clean, report_commit, report_diff, report_reset, report_restore, report_rm, report_stash, report_status_progress, report_switch},
    types::{RepoSnapshot, Verbosity},
};
use std::collections::BTreeSet;

pub(super) fn report(command: &str, args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    // Whatever the command, a newly stopped operation with conflicts gets the full report.
    if !a.unmerged.is_empty() && (b.unmerged != a.unmerged || b.operation != a.operation) && ["merge", "pull", "rebase", "cherry-pick", "revert", "stash"].contains(&command) {
        report_conflicts(a, b);
        return;
    }
    match command {
        "add" => report_add(args, b, a, ok),
        "commit" => report_commit(args, b, a),
        "merge" => report_merge(args, b, a, ok, false),
        "pull" => report_pull(args, b, a, ok),
        "fetch" => report_fetch(b, a, ok),
        "push" => report_push(args, b, a, ok),
        "rebase" => report_rebase(args, b, a, ok),
        "cherry-pick" | "revert" => report_pick(command, args, b, a),
        "reset" => report_reset(args, b, a, ok),
        "restore" => report_restore(args, b, a, ok),
        "checkout" => report_checkout(args, b, a, ok),
        "switch" => report_switch(args, b, a, ok),
        "stash" => report_stash(args, b, a, ok),
        "rm" => report_rm(args, b, a),
        "clean" => report_clean(args, b, a, ok),
        "tag" => report_tag(args, b, a),
        "branch" => report_branch(args, b, a),
        "diff" => report_diff(args, b, ok),
        "status" => report_status_progress(a),
        "log" => report_log(args, a),
        _ => {}
    }
}

pub(super) fn teach_changes(b: &RepoSnapshot, a: &RepoSnapshot) {
    if level() < Verbosity::Teach {
        return;
    }
    let names: BTreeSet<&String> = b.refs.keys().chain(a.refs.keys()).collect();
    for name in names {
        match (b.refs.get(name), a.refs.get(name)) {
            (Some(x), Some(y)) if x != y => teach(&format!("{} moved {} -> {}", name, short(x), short(y))),
            (None, Some(y)) => teach(&format!("created {} at {}", name, short(y))),
            (Some(x), None) => teach(&format!("deleted {} (was {})", name, short(x))),
            _ => {}
        }
    }
    if b.head_branch != a.head_branch || (a.head_branch.is_none() && b.head != a.head) {
        let describe = |s: &RepoSnapshot| s.head_branch.clone().map(|br| format!("ref: refs/heads/{}", br)).unwrap_or_else(|| s.head.clone().map(|h| short(&h).to_string()).unwrap_or_default());
        teach(&format!(".git/HEAD now says '{}' (was '{}')", describe(a), describe(b)));
    }
    let changed = b.index.iter().filter(|(p, h)| a.index.get(*p) != Some(h)).count() + a.index.keys().filter(|p| !b.index.contains_key(*p)).count();
    if changed > 0 {
        teach(&format!("rewrote .git/index: {} changed (it now lists {} files)", plural(changed, "entry", "entries"), a.index.len()));
    }
}

/// Paths whose staged (stage 0) entry differs between two snapshots.
fn index_diff(b: &RepoSnapshot, a: &RepoSnapshot) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for (p, h) in &a.index {
        if b.index.get(p) != Some(h) {
            out.insert(p.clone());
        }
    }
    for p in b.index.keys() {
        if !a.index.contains_key(p) {
            out.insert(p.clone());
        }
    }
    out.into_iter().collect()
}

/// The branch's upstream ref; like rgit's `pull`/`push`, falls back to
/// `refs/remotes/<branch.<b>.remote or origin>/<b>` when that exists.
fn upstream_of(branch: &str) -> Option<String> {
    crate::references::for_each_ref::engine::upstream::branch_upstream(branch).or_else(|| {
        let remote = crate::core::config::get_config_value(&format!("branch.{}.remote", branch)).ok().flatten().unwrap_or_else(|| "origin".into());
        let candidate = format!("refs/remotes/{}/{}", remote, branch);
        crate::core::refs::read_ref(&candidate).ok().flatten().map(|_| candidate)
    })
}

fn not_pushed_line(a: &RepoSnapshot) -> Option<String> {
    let branch = a.head_branch.as_ref()?;
    let head = a.head.as_ref()?;
    match upstream_of(branch) {
        Some(up) => {
            let up_hash = a.refs.get(&up)?;
            let ahead = commits_between(Some(up_hash), head);
            let short_up = crate::references::for_each_ref::engine::upstream::shorten_unambiguous_ref(&up);
            Some(if ahead == 0 { format!("in sync with {}", short_up) } else { format!("{} not pushed to {} yet", plural(ahead, "commit", "commits"), short_up) })
        }
        None => None,
    }
}

fn file_summary(old: Option<&str>, new: Option<&str>) -> (String, Vec<String>, Vec<String>, Vec<String>) {
    let (added, modified, deleted, plus, minus) = tree_changes(old, new);
    let files = added.len() + modified.len() + deleted.len();
    (format!("{}, +{} -{}", plural(files, "file", "files"), plus, minus), added, modified, deleted)
}

fn file_breakdown(added: &[String], modified: &[String], deleted: &[String]) {
    if !added.is_empty() {
        detail(&format!("new:     {}", list(added, 5)));
    }
    if !modified.is_empty() {
        detail(&format!("changed: {}", list(modified, 5)));
    }
    if !deleted.is_empty() {
        detail(&format!("deleted: {}", list(deleted, 5)));
    }
}

fn risk_warnings(paths: &[String]) {
    let (secrets, build, large) = scan_risks(paths);
    for (path, what) in secrets.iter().take(5) {
        warn(&format!("'{}' looks like it contains {}. Once committed and pushed, it's in the history for good.", path, what));
    }
    if !secrets.is_empty() {
        next("unstage with `rgit restore --staged <file>` and add it to .gitignore.");
    }
    if !build.is_empty() {
        warn(&format!("{} look like build output: {}.", plural(build.len(), "file", "files"), list(&build, 3)));
        next("add the build folder to .gitignore (e.g. `echo target/ >> .gitignore`) and `rgit restore --staged` them.");
    }
    for (path, size) in large {
        warn(&format!("'{}' is {:.1} MiB; GitHub warns above 50 MiB and refuses files over 100 MiB.", path, size as f64 / (1024.0 * 1024.0)));
    }
}

// --- add ---------------------------------------------------------------------------------

fn head_move(b: &RepoSnapshot, a: &RepoSnapshot) -> Option<String> {
    let (old, new) = (b.head.clone()?, a.head.clone()?);
    if old == new {
        return None;
    }
    Some(if is_ancestor(&new, &old) {
        format!("HEAD moved {} back ({} -> {})", plural(commits_between(Some(&new), &old), "commit", "commits"), short(&old), short(&new))
    } else {
        format!("HEAD moved {} -> {}", short(&old), short(&new))
    })
}
