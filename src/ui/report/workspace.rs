//! Reports for working-tree commands: add, commit, status, reset, restore, switch, checkout, stash, rm, clean, diff.

use crate::core::commit::commit_parents;
use crate::ui::{
    {danger, detail, has, level, list, next, note, plural, positionals, short, warn},
    narrate::{commit_info, commit_tree_map, commits_between, exists, first_marker_line, ignore_reason, ignored_files_under, is_ancestor, load_conflict_list, scan_risks, suggest, under_any},
    report::{file_breakdown, file_summary, head_move, index_diff, not_pushed_line, risk_warnings, upstream_of},
    types::{RepoSnapshot, Verbosity},
};
use std::collections::BTreeSet;

pub(super) fn report_add(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let specs = positionals(args, &[]);
    let changed = index_diff(b, a);
    // Paths given that don't exist at all.
    let mut candidates: Vec<String> = b.index.keys().cloned().collect();
    candidates.extend(b.untracked.clone().unwrap_or_default());
    for spec in &specs {
        let s = spec.trim_end_matches('/');
        if s != "." && !s.is_empty() && !exists(s) && !b.index.keys().any(|p| under_any(p, std::slice::from_ref(&s.to_string()))) {
            match suggest(s, &candidates) {
                Some(c) => note(&format!("'{}' doesn't exist. Did you mean '{}'?", s, c)),
                None => note(&format!("'{}' doesn't exist here (paths are relative to where you run rgit).", s)),
            }
        }
    }
    if changed.is_empty() && b.unmerged == a.unmerged {
        if ok == Some(false) {
            return;
        }
        // One specific path: say exactly why it wasn't added.
        if let [spec] = specs.as_slice() {
            let s = spec.trim_end_matches('/');
            if s != "." && exists(s) {
                if let Some((source, line_no, pattern)) = ignore_reason(s) {
                    note(&format!("'{}' wasn't added: it's ignored by {}:{} ({}).", s, source, line_no, pattern));
                    next("remove that line from .gitignore if the file should be tracked.");
                    return;
                }
                if b.index.contains_key(s) && !b.worktree_changes.as_ref().is_some_and(|c| c.iter().any(|(_, p)| p == s)) {
                    let staged = b.staged.iter().any(|p| p == s);
                    note(&format!("Nothing to add: '{}' has no changes{}.", s, if staged { " since you last staged it" } else { " since the last commit" }));
                    return;
                }
            }
        }
        let ignored = ignored_files_under(&specs);
        let reason = if !b.staged.iter().any(|p| under_any(p, &specs)) {
            "no changes here since the last commit".to_string()
        } else {
            format!("everything here is already staged ({} ready: `rgit commit`)", plural(b.staged.len(), "file", "files"))
        };
        let ignored_note = if ignored.is_empty() { String::new() } else { format!("; {} skipped, e.g. {}", plural(ignored.len(), "ignored file", "ignored files"), ignored[0]) };
        note(&format!("Nothing to add: {}{}.", reason, ignored_note));
        return;
    }
    // Conflicts marked resolved: warn about leftover markers, show progress.
    let resolved: Vec<String> = b.unmerged.keys().filter(|p| !a.unmerged.contains_key(*p)).cloned().collect();
    for path in &resolved {
        if let Some(n) = first_marker_line(path) {
            warn(&format!("'{}' is now marked resolved but still contains conflict markers (line {}).", path, n));
        }
    }
    if !resolved.is_empty() {
        report_status_progress(a);
    }
    let fresh: Vec<String> = changed.iter().filter(|p| a.index.contains_key(*p)).cloned().collect();
    for path in fresh.iter().filter(|p| !b.index.contains_key(*p)) {
        if let Some((source, line_no, pattern)) = ignore_reason(path) {
            warn(&format!("'{}' is ignored by {}:{} ({}) but was staged anyway; git would refuse without -f.", path, source, line_no, pattern));
        }
    }
    risk_warnings(&fresh);
    let new = changed.iter().filter(|p| !b.index.contains_key(*p)).count();
    let removed = changed.iter().filter(|p| !a.index.contains_key(*p)).count();
    detail(&format!("Staged {} ({} new, {} changed, {} removed). `rgit commit` records them; `rgit restore --staged <file>` unstages.", plural(changed.len(), "file", "files"), new, changed.len() - new - removed, removed));
}

// --- commit ------------------------------------------------------------------------------

pub(super) fn report_commit(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
    let amend = has(args, &["--amend"]);
    if a.head == b.head {
        if !a.unmerged.is_empty() {
            return;
        }
        if b.staged.is_empty() && !has(args, &["-a", "--all"]) {
            let changes = b.worktree_changes.clone().unwrap_or_default();
            let untracked = b.untracked.clone().unwrap_or_default();
            if !changes.is_empty() {
                note(&format!("Nothing committed: nothing is staged, but {} changed ({}).", plural(changes.len(), "tracked file", "tracked files"), list(&changes.iter().map(|c| c.1.clone()).collect::<Vec<_>>(), 3)));
                next("`rgit add <file>` to pick what goes in, or `rgit commit -a` to commit all tracked changes.");
            } else if !untracked.is_empty() {
                note(&format!("Nothing committed: {} not tracked yet ({}).", plural(untracked.len(), "new file is", "new files are"), list(&untracked, 3)));
                next("`rgit add <file>` first, then commit.");
            } else {
                detail("Nothing to commit: your files match the last commit exactly.");
            }
        } else if !b.staged.is_empty() {
            note("No commit was made (empty message or the commit was aborted); your staged changes are still staged.");
        }
        return;
    }
    let Some(new) = a.head.clone() else { return };
    let branch_label = a.head_branch.clone().unwrap_or_else(|| "a detached HEAD".into());
    if amend {
        let Some(old) = b.head.clone() else { return };
        let same_tree = commit_tree_map(Some(&old)) == commit_tree_map(Some(&new));
        let same_message = commit_info(&old).map(|i| i.0) == commit_info(&new).map(|i| i.0);
        let what = match (same_tree, same_message) {
            (true, false) => "message changed, same files".to_string(),
            (true, true) => "same files and message (only author/date details changed)".to_string(),
            (false, _) => {
                let parent = crate::core::object_store::read_object(&new).ok().and_then(|(_, c)| commit_parents(&String::from_utf8_lossy(&c)).into_iter().next());
                let (summary, ..) = file_summary(parent.as_deref(), Some(&new));
                format!("{}it now changes {}", if same_message { "same message; " } else { "new message; " }, summary)
            }
        };
        note(&format!("Replaced {} with {} on {}: {}.", short(&old), short(&new), branch_label, what));
        if let Some(branch) = &a.head_branch {
            if let Some(up_hash) = upstream_of(branch).and_then(|u| a.refs.get(&u).cloned()) {
                if is_ancestor(&old, &up_hash) {
                    warn("the commit you amended was already pushed; pushing now needs `rgit push -f` (only if nobody built on it).");
                }
            }
        }
        detail("The old commit isn't gone: `rgit reflog` lists it (HEAD@{1}) if you need it back.");
    } else {
        let parent = crate::core::object_store::read_object(&new).ok().and_then(|(_, c)| commit_parents(&String::from_utf8_lossy(&c)).into_iter().next());
        let (summary, added, modified, deleted) = file_summary(parent.as_deref(), Some(&new));
        let pushed = not_pushed_line(a).map(|s| format!("; {}", s)).unwrap_or_default();
        note(&format!("Recorded {} on {}: {}{}.", short(&new), branch_label, summary, pushed));
        file_breakdown(&added, &modified, &deleted);
        if pushed.is_empty() && a.head_branch.is_some() {
            detail("Not pushed anywhere yet: this branch has no upstream (`rgit push -u` sets one).");
        }
        let touched: Vec<String> = added.iter().chain(modified.iter()).cloned().collect();
        for path in &touched {
            if let Some(n) = first_marker_line(path) {
                warn(&format!("'{}' was committed with conflict markers in it (line {}).", path, n));
            }
        }
        let (_, _, large) = scan_risks(&touched);
        for (path, size) in large {
            warn(&format!("'{}' ({:.1} MiB) is now in the history; GitHub refuses files over 100 MiB.", path, size as f64 / (1024.0 * 1024.0)));
        }
    }
    if a.head_branch.is_none() {
        warn(&format!("you committed on a detached HEAD: {} isn't on any branch and is easy to lose.", short(&new)));
        next("`rgit switch -c <name>` keeps it on a new branch.");
    }
}

// --- merge / pull ------------------------------------------------------------------------

/// "2 of 3 conflicted files resolved" while an operation that stopped on conflicts is in progress.
pub(super) fn report_status_progress(a: &RepoSnapshot) {
    let all = load_conflict_list();
    if all.is_empty() {
        return;
    }
    let left: Vec<String> = all.iter().filter(|p| a.unmerged.contains_key(*p)).cloned().collect();
    let op = match a.operation.as_deref() {
        Some("merge") | None => "merge",
        Some(o) => o,
    };
    if left.is_empty() {
        note(&if all.len() == 1 { "The conflicted file is resolved.".to_string() } else { format!("All {} conflicted files are resolved.", all.len()) });
        if op == "stash" {
            next("the stashed changes are in; the stash entry was kept: `rgit stash drop` when you're done with it.");
            crate::ui::narrate::clear_stash_conflict();
        } else {
            next(&format!("`rgit {} --continue` to finish.", op));
        }
    } else {
        note(&format!("Conflicts: {} of {} resolved; still to do: {}.", all.len() - left.len(), all.len(), list(&left, 4)));
    }
}

// --- reset / restore / checkout / switch / stash / rm / clean -----------------------------

pub(super) fn report_reset(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    if b.operation.as_deref() == Some("stash") && a.unmerged.is_empty() {
        crate::ui::narrate::clear_stash_conflict();
    }
    let paths = positionals(args, &[]);
    let hard = has(args, &["--hard"]);
    let soft = has(args, &["--soft"]);
    let index_changed = index_diff(b, a);
    let tree_lost = b.worktree_changes.clone().unwrap_or_default();
    let tree_now = a.worktree_changes.clone().unwrap_or_default();
    let moved = head_move(b, a);
    if moved.is_none() && index_changed.is_empty() && (!hard || tree_lost == tree_now) {
        if ok == Some(true) {
            note("Nothing to reset: HEAD, the index and your files already match.");
        }
        return;
    }
    // `reset <commit> -- <path>` / `reset <path>`: unstaging only.
    if moved.is_none() && !hard && !soft && !paths.is_empty() {
        note(&format!("Unstaged {} (your file contents are untouched).", list(&index_changed, 4)));
        return;
    }
    let moved_text = moved.unwrap_or_else(|| "HEAD stayed where it was".into());
    if hard {
        let staged_lost = b.staged.len();
        let files_lost = tree_lost.len();
        if files_lost + staged_lost > 0 {
            danger(&format!("{}; discarded {} and {}. Recoverable: the commits yes (`rgit reset --hard ORIG_HEAD`), the uncommitted file changes no.", moved_text, plural(files_lost, "changed file", "changed files"), plural(staged_lost, "staged file", "staged files")));
        } else {
            note(&format!("{}; no uncommitted work was lost. Undo: `rgit reset --hard ORIG_HEAD`.", moved_text));
        }
    } else if soft {
        note(&format!("{}. Index and files untouched: the undone commits' changes are now staged, ready to re-commit.", moved_text));
    } else {
        let unstaged = a.worktree_changes.as_ref().map(|c| c.len()).unwrap_or(0);
        if unstaged > 0 {
            note(&format!("{}. Your files are untouched; {} now unstaged (`rgit add` to stage again).", moved_text, plural(unstaged, "file's changes are", "files' changes are")));
        } else {
            note(&format!("{}. Your files are untouched and match the new HEAD.", moved_text));
        }
    }
}

pub(super) fn report_restore(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let staged = has(args, &["--staged", "-S"]);
    let specs = positionals(args, &["-s", "--source"]);
    let before: BTreeSet<String> = b.worktree_changes.clone().unwrap_or_default().into_iter().map(|c| c.1).collect();
    let after: BTreeSet<String> = a.worktree_changes.clone().unwrap_or_default().into_iter().map(|c| c.1).collect();
    let discarded: Vec<String> = before.difference(&after).cloned().collect();
    let unstaged = index_diff(b, a);
    if !discarded.is_empty() && !has(args, &["-p", "--patch"]) {
        danger(&format!("Discarded your changes in {} (not recoverable).", list(&discarded, 4)));
    }
    if staged && !unstaged.is_empty() {
        note(&format!("Unstaged {} (file contents untouched).", list(&unstaged, 4)));
    }
    if discarded.is_empty() && unstaged.is_empty() && ok == Some(true) {
        let (what, verb) = if specs.len() == 1 { (format!("'{}'", specs[0]), "matches") } else { ("these files".to_string(), "match") };
        note(&format!("Nothing to restore: {} already {} {}.", what, verb, if staged { "HEAD (nothing is staged)" } else { "the index" }));
    }
}

fn switch_summary(b: &RepoSnapshot, a: &RepoSnapshot, created: bool) {
    if a.head_branch.is_none() {
        let Some(head) = &a.head else { return };
        note(&format!("You're on a detached HEAD at {}: new commits here won't belong to any branch.", short(head)));
        next(&format!("`rgit switch -c <name>` to start a branch here; `rgit switch {}` to go back.", b.head_branch.clone().unwrap_or_else(|| "-".into())));
        return;
    }
    let branch = a.head_branch.clone().unwrap();
    let head = a.head.clone();
    let mut parts = Vec::new();
    if created {
        parts.push(format!("new branch '{}' starts at {}", branch, head.as_deref().map(short).unwrap_or("")));
    }
    if let Some(h) = &head {
        let compare = upstream_of(&branch).and_then(|u| a.refs.get(&u).map(|x| (crate::references::for_each_ref::engine::upstream::shorten_unambiguous_ref(&u), x.clone())))
            .or_else(|| b.head_branch.clone().filter(|p| p != &branch).and_then(|p| b.head.clone().map(|x| (p, x))));
        if let Some((name, other)) = compare.filter(|_| !created) {
            let (ahead, behind) = (commits_between(Some(&other), h), commits_between(Some(h), &other));
            if ahead + behind > 0 {
                let mut rel = Vec::new();
                if ahead > 0 {
                    rel.push(format!("{} ahead of", plural(ahead, "commit", "commits")));
                }
                if behind > 0 {
                    rel.push(format!("{} behind", plural(behind, "commit", "commits")));
                }
                parts.push(format!("{} '{}'", rel.join(" and "), name));
            } else {
                parts.push(format!("same commit as '{}'", name));
            }
        }
    }
    let carried = a.worktree_changes.as_ref().map(|c| c.len()).unwrap_or(0);
    if carried > 0 {
        parts.push(format!("your uncommitted changes ({}) came along", plural(carried, "file", "files")));
    }
    if !parts.is_empty() {
        note(&format!("On '{}': {}.", branch, parts.join("; ")));
    }
}

pub(super) fn report_switch(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let created = has(args, &["-c", "-C", "--create", "--force-create"]);
    if b.head_branch != a.head_branch || b.head != a.head {
        switch_summary(b, a, created);
    } else if ok != Some(true) {
        let target = positionals(args, &[]).into_iter().next();
        let dirty = b.worktree_changes.as_ref().is_some_and(|c| !c.is_empty()) || !b.staged.is_empty();
        if dirty && target.is_some() && target != b.head_branch {
            note("Didn't switch: your uncommitted changes would be overwritten.");
            next("commit them, `rgit stash` them, or add `-f` to throw them away.");
        }
    }
}

pub(super) fn report_checkout(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let pos = positionals(args, &["-b", "-B"]);
    let path_mode = args.iter().any(|a| a == "--") || (pos.len() == 1 && exists(&pos[0]) && !b.refs.contains_key(&format!("refs/heads/{}", pos[0])));
    if path_mode {
        report_restore(args, b, a, ok);
        if level() >= Verbosity::Explain {
            detail("`rgit restore <file>` does the same thing with a clearer name.");
        }
        return;
    }
    report_switch(args, b, a, ok);
    if (b.head_branch != a.head_branch) && level() >= Verbosity::Explain {
        detail("`rgit switch <branch>` does the same thing and can't be confused with restoring files.");
    }
}

pub(super) fn report_stash(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let sub = positionals(args, &["-m", "--message"]).into_iter().next().unwrap_or_else(|| "push".into());
    let with_untracked = has(args, &["-u", "--include-untracked", "-a", "--all"]);
    let untracked = b.untracked.clone().unwrap_or_default();
    match sub.as_str() {
        "push" | "save" => {
            if a.stash_count > b.stash_count {
                let changed = b.worktree_changes.as_ref().map(|c| c.len()).unwrap_or(0);
                let mut what = Vec::new();
                for (n, one, many) in [(changed, "changed file", "changed files"), (b.staged.len(), "staged file", "staged files"), (if with_untracked { untracked.len() } else { 0 }, "untracked file", "untracked files")] {
                    if n > 0 {
                        what.push(plural(n, one, many));
                    }
                }
                note(&format!("Saved {} as stash@{{0}}; your working tree is clean now.", what.join(" + ")));
                next("`rgit stash pop` brings them back.");
                if !with_untracked && !untracked.is_empty() {
                    note(&format!("{} not stashed and still here ({}); `rgit stash -u` includes them.", plural(untracked.len(), "untracked file was", "untracked files were"), list(&untracked, 3)));
                }
            } else if ok == Some(true) && !with_untracked && !untracked.is_empty() {
                note(&format!("Nothing stashed: only untracked files changed ({}); `rgit stash -u` stashes them.", list(&untracked, 3)));
            }
        }
        "pop" if a.stash_count < b.stash_count => {
            let restored = a.worktree_changes.as_ref().map(|c| c.len()).unwrap_or(0) + a.staged.len();
            let left = if a.stash_count == 0 { "no stashes left".to_string() } else { format!("{} left", plural(a.stash_count, "stash", "stashes")) };
            note(&format!("Restored {} from the stash; {}.", plural(restored, "change", "changes"), left));
        }
        "apply" if ok == Some(true) => {
            note("Applied the stash; it's still saved (`rgit stash drop` removes it, `pop` does both).");
        }
        "drop" if a.stash_count < b.stash_count => {
            detail("Dropped stashes can't be listed any more; `rgit fsck` may still find the commit for a while.");
        }
        _ => {}
    }
}

pub(super) fn report_rm(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
    let specs = positionals(args, &[]);
    for spec in &specs {
        if !b.index.keys().any(|p| under_any(p, std::slice::from_ref(spec))) {
            if exists(spec) {
                note(&format!("'{}' isn't tracked, so there's nothing to remove from rgit (the file stays on disk; delete it normally).", spec));
            } else {
                let candidates: Vec<String> = b.index.keys().cloned().collect();
                if let Some(c) = suggest(spec, &candidates) {
                    note(&format!("'{}' isn't tracked. Did you mean '{}'?", spec, c));
                }
            }
        }
    }
    let removed = index_diff(b, a);
    if !removed.is_empty() {
        if has(args, &["--cached"]) {
            note(&format!("Stopped tracking {}; the files stay on disk (add them to .gitignore to keep them out).", list(&removed, 4)));
        } else {
            detail(&format!("Removed {} from disk and the index; `rgit commit` records the deletion.", list(&removed, 4)));
        }
    }
}

pub(super) fn report_clean(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    if has(args, &["-n", "--dry-run"]) || ok == Some(false) {
        return;
    }
    let before: BTreeSet<String> = b.untracked.clone().unwrap_or_default().into_iter().collect();
    let after: BTreeSet<String> = a.untracked.clone().unwrap_or_default().into_iter().collect();
    let removed = before.difference(&after).count();
    if removed > 0 {
        danger(&format!("Removed {} (they were never committed, so this can't be undone).", plural(removed, "untracked file", "untracked files")));
    } else if ok == Some(true) && !has(args, &["-x", "-X"]) {
        note("Nothing to clean: there are no untracked files.");
    }
}

// --- tag / branch / diff / clone ---------------------------------------------------------

pub(super) fn report_diff(args: &[String], b: &RepoSnapshot, ok: Option<bool>) {
    if ok != Some(true) || !positionals(args, &[]).is_empty() || args.iter().any(|a| a.starts_with("--stat") || a == "--name-only" || a == "--name-status" || a == "--quiet" || a == "--exit-code") {
        return;
    }
    let unstaged = b.worktree_changes.as_ref().map(|c| c.len()).unwrap_or(0);
    if has(args, &["--cached", "--staged"]) {
        if b.staged.is_empty() {
            note(&format!("No staged changes: the index matches the last commit{}.", if unstaged > 0 { format!(" ({} unstaged: plain `rgit diff`)", plural(unstaged, "file", "files")) } else { String::new() }));
        }
    } else if unstaged == 0 {
        let staged = b.staged.len();
        note(&format!("No unstaged changes: your files match the index{}.", if staged > 0 { format!(" ({} staged: `rgit diff --cached`)", plural(staged, "file is", "files are")) } else { String::new() }));
    }
}
