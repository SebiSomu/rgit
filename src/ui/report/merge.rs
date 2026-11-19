//! Reports for merge, rebase, cherry-pick/revert and conflicts, with the next steps to take.

use crate::core::commit::commit_parents;
use crate::ui::{
    {detail, has, line, next, note, plural, positionals, short, warn},
    narrate::{commit_info, commit_tree_map, commits_between, conflict_kind, file_conflict_hunks, is_ancestor, last_change, relative_time, save_conflict_list, their_commit},
    report::{file_breakdown, file_summary, index_diff, not_pushed_line, upstream_of},
    types::RepoSnapshot,
};

fn merge_target_name(args: &[String]) -> Option<String> {
    positionals(args, &["-m", "-F", "-s", "-X", "--strategy", "--strategy-option", "--file", "--message"]).into_iter().next()
}

pub(super) fn report_merge(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>, quiet_noop: bool) {
    let current = a.head_branch.clone().unwrap_or_else(|| "HEAD".into());
    if has(args, &["--abort"]) {
        if b.operation.is_some() && a.operation.is_none() {
            note(&format!("Merge aborted: '{}' and your files are back to how they were before the merge.", current));
        }
        return;
    }
    let target = merge_target_name(args);
    if a.head == b.head {
        if a.operation.as_deref() == Some("merge") && a.unmerged.is_empty() {
            if has(args, &["--no-commit"]) {
                note("Merged, but not committed yet (--no-commit): review the result, then `rgit commit` to finish.");
            } else {
                note("The merge is done in your files and index but not committed yet.");
                next("`rgit commit` finishes it.");
            }
            next("`rgit merge --abort` undoes it.");
        } else if crate::core::repo::path(".git/SQUASH_MSG").exists() && !index_diff(b, a).is_empty() {
            note(&format!("Squashed {} into your index; nothing is committed yet.", target.as_deref().unwrap_or("the branch")));
            next("`rgit commit` records it as one commit.");
        } else if let (Some(t), Some(head)) = (&target, &a.head) {
            if let Ok(hash) = crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", t)) {
                if is_ancestor(&hash, head) && !quiet_noop && ok != Some(false) {
                    note(&format!("Already up to date: '{}' is already part of '{}', nothing to merge.", t, current));
                }
            }
        }
        return;
    }
    let (Some(old), Some(new)) = (b.head.clone(), a.head.clone()) else { return };
    let parents = crate::core::object_store::read_object(&new).ok().map(|(_, c)| commit_parents(&String::from_utf8_lossy(&c))).unwrap_or_default();
    let (summary, added, modified, deleted) = file_summary(Some(&old), Some(&new));
    let name = target.or_else(|| commit_info(&new).and_then(|(subject, ..)| subject.split('\'').nth(1).map(String::from))).unwrap_or_else(|| "the other branch".into());
    if parents.len() >= 2 && parents[0] == old {
        let commits = commits_between(Some(&old), &parents[1]);
        note(&format!("Merged '{}' into '{}': {}, {}.", name, current, plural(commits, "commit", "commits"), summary));
    } else if is_ancestor(&old, &new) {
        let commits = commits_between(Some(&old), &new);
        note(&format!("Fast-forward: '{}' moved {} -> {} ({}; {}). No merge commit was needed.", current, short(&old), short(&new), plural(commits, "commit", "commits"), summary));
        detail("Your branch had nothing new of its own, so it simply moved forward to the other branch's tip.");
    } else {
        return;
    }
    file_breakdown(&added, &modified, &deleted);
    if let Some(line) = not_pushed_line(a) {
        detail(&format!("'{}' is now {}.", current, line));
    }
}

pub(super) fn report_rebase(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let branch = a.head_branch.clone().unwrap_or_else(|| "HEAD".into());
    if has(args, &["--abort"]) {
        if b.operation.is_some() && a.operation.is_none() {
            note(&format!("Rebase aborted: '{}' is back at {}, as before the rebase.", branch, a.head.as_deref().map(short).unwrap_or("")));
        }
        return;
    }
    if a.operation.as_deref() == Some("rebase") {
        if a.unmerged.is_empty() && b.head != a.head {
            note("Rebase paused (edit): change what you need, `rgit commit --amend`, then `rgit rebase --continue`.");
        }
        return;
    }
    if a.head == b.head || ok == Some(false) {
        return;
    }
    let (Some(old), Some(new)) = (b.head.clone().or_else(|| b.refs.get(&format!("refs/heads/{}", branch)).cloned()), a.head.clone()) else { return };
    if b.operation.as_deref() == Some("rebase") {
        note(&format!("Rebase finished: '{}' is now at {}.", branch, short(&new)));
    } else if is_ancestor(&old, &new) && commits_between(Some(&old), &new) > 0 && commit_tree_map(Some(&old)) != commit_tree_map(Some(&new)) {
        note(&format!("Fast-forwarded '{}' to {}: it had no commits of its own to replay.", branch, short(&new)));
        return;
    } else {
        note(&format!("Rebased '{}': its commits were replayed and now sit on top of the new base (HEAD {} -> {}).", branch, short(&old), short(&new)));
    }
    detail("The old commits still exist: `rgit reset --hard ORIG_HEAD` undoes the rebase.");
    if let Some(up_hash) = upstream_of(&branch).and_then(|u| a.refs.get(&u).cloned()) {
        if !is_ancestor(&up_hash, &new) {
            warn(&format!("'{}' was already pushed; the rebased commits replace those, so pushing needs `rgit push -f`.", branch));
        }
    }
}

pub(super) fn report_pick(command: &str, args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
    let pick = command == "cherry-pick";
    if has(args, &["--abort", "--quit"]) {
        if b.operation.is_some() && a.operation.is_none() {
            note(&format!("{} stopped: your branch is back to how it was.", if pick { "Cherry-pick" } else { "Revert" }));
        }
        return;
    }
    if a.operation.as_deref() == Some(command) && a.unmerged.is_empty() && a.staged.is_empty() && a.head == b.head {
        if pick {
            note("Empty after applying: these changes are already in HEAD, so no commit was made.");
            next("`rgit cherry-pick --skip` moves on (or `--allow-empty` keeps an empty commit).");
        } else {
            note("Nothing to revert: those changes are not in HEAD any more.");
        }
        return;
    }
    let (Some(old), Some(new)) = (b.head.clone(), a.head.clone()) else { return };
    if old == new {
        if has(args, &["-n", "--no-commit"]) && !a.staged.is_empty() {
            note(&format!("Applied to your index and files without committing ({} staged): `rgit commit` when ready.", plural(a.staged.len(), "file", "files")));
        }
        return;
    }
    let count = commits_between(Some(&old), &new);
    let branch = a.head_branch.clone().unwrap_or_else(|| "HEAD".into());
    if pick {
        note(&format!("Picked {} onto '{}': now at {}.", plural(count, "commit", "commits"), branch, short(&new)));
    } else {
        note(&format!("Added {} on '{}' that undo the reverted changes (history is kept, nothing was deleted).", plural(count, "commit", "commits"), branch));
    }
}

// --- conflicts ---------------------------------------------------------------------------

/// Conflict summary lines for one file: hunks, kind and who changed it on each side.
pub fn conflict_file_lines(path: &str, mask: u8, operation: &str, ours_tip: Option<&str>) -> Vec<String> {
    let hunks = file_conflict_hunks(path);
    let mut out = Vec::new();
    let place = if hunks.is_empty() {
        String::new()
    } else {
        let ranges: Vec<String> = hunks.iter().take(4).map(|h| format!("{}-{}", h.start, h.end)).collect();
        format!("conflict in {} (lines {}{})  ", plural(hunks.len(), "place", "places"), ranges.join(", "), if hunks.len() > 4 { ", ..." } else { "" })
    };
    out.push(format!("{:<24} {}{}", path, place, conflict_kind(mask)));
    let theirs_label = hunks.first().map(|h| h.theirs_label.clone());
    let who = |tip: Option<String>, side: &str, label: String| {
        tip.and_then(|t| last_change(&t, path)).map(|(_, subject, author, when)| format!("    {:<7} {:<18} '{}' ({}, {})", side, label, subject, author, relative_time(when)))
    };
    let ours_label = hunks.first().map(|h| h.ours_label.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| "HEAD".into());
    if let Some(l) = who(ours_tip.map(String::from), "ours:", ours_label) {
        out.push(l);
    }
    let theirs_tip = their_commit(operation, theirs_label.as_deref());
    let label = theirs_label.filter(|l| !l.is_empty()).unwrap_or_else(|| theirs_tip.as_deref().map(short).unwrap_or("").to_string());
    if let Some(l) = who(theirs_tip, "theirs:", label) {
        out.push(l);
    }
    out
}

pub(super) fn report_conflicts(a: &RepoSnapshot, b: &RepoSnapshot) {
    let operation = a.operation.clone().unwrap_or_else(|| "merge".into());
    let title = match operation.as_str() {
        "rebase" => {
            let total: usize = std::fs::read_to_string(crate::core::repo::path(crate::merge::rebase::engine::REBASE_TOTAL_PATH)).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
            let left = std::fs::read_to_string(crate::core::repo::path(crate::merge::rebase::engine::REBASE_TODO_PATH)).map(|t| t.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count()).unwrap_or(0);
            let current = (total.saturating_sub(left) + 1).min(total);
            if total > 0 { format!("Rebase stopped at commit {} of {}", current, total) } else { "Rebase stopped".to_string() }
        }
        "cherry-pick" => "Cherry-pick stopped".to_string(),
        "revert" => "Revert stopped".to_string(),
        "stash" => "Stash apply stopped".to_string(),
        _ => if b.stash_count != a.stash_count || a.operation.is_none() { "Applying stopped".to_string() } else { "Merge stopped".to_string() },
    };
    // What merged cleanly is what is staged against the (new) HEAD, conflicts aside.
    let head_tree = commit_tree_map(a.head.as_deref());
    let clean: Vec<String> = a.staged.iter().filter(|p| !a.unmerged.contains_key(*p)).cloned().collect();
    let added = clean.iter().filter(|p| !head_tree.contains_key(*p) && a.index.contains_key(*p)).count();
    let _ = b;
    let conflicted: Vec<String> = a.unmerged.keys().cloned().collect();
    let mut head = format!("{}: {} conflict", title, plural(conflicted.len(), "file", "files"));
    if clean.len() > added {
        head.push_str(&format!(", {} merged cleanly", clean.len() - added));
    }
    if added > 0 {
        head.push_str(&format!(", {} added", added));
    }
    note(&format!("{}.", head));
    for path in conflicted.iter().take(20) {
        for l in conflict_file_lines(path, a.unmerged[path], &operation, a.head.as_deref()) {
            line(&l);
        }
    }
    if conflicted.len() > 20 {
        line(&format!("... and {} more (`rgit conflicts` lists them all)", conflicted.len() - 20));
    }
    next("open each file, keep the code you want, delete the <<<<<<< ======= >>>>>>> lines,");
    for l in conflict_next_steps(&operation) {
        next(&l);
    }
    detail("`rgit conflicts --explain <file>` shows each conflict as YOURS / THEIRS side by side.");
    save_conflict_list(&conflicted);
}

/// What to do after editing the conflicted files, and how to give up, for each operation.
// Used by the conflict report and `rgit conflicts`.
pub fn conflict_next_steps(operation: &str) -> Vec<String> {
    let op = if operation == "merge" || operation == "bisect" { "merge" } else { operation };
    match op {
        "stash" => vec![
            "then `rgit add <file>` for each one. There is nothing to continue: the stash entry was kept,".to_string(),
            "so `rgit stash drop` once you're happy with the result.".to_string(),
            "Changed your mind? `rgit reset --hard` undoes the apply (the tree was clean before it); the stash is still there.".to_string(),
        ],
        "rebase" => vec![
            "then `rgit add <file>` for each one and `rgit rebase --continue`.".to_string(),
            "`rgit rebase --skip` drops this commit; `rgit rebase --abort` returns to before the rebase.".to_string(),
        ],
        _ => vec![
            format!("then `rgit add <file>` for each one and `rgit {} --continue`.", op),
            format!("Changed your mind? `rgit {} --abort` puts everything back as it was.", op),
        ],
    }
}
