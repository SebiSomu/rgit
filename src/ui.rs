//! rgit's narration layer ("the git that explains itself", ideas file C.1 + C.10).
//!
//! Every state-changing command is wrapped: `begin` takes a snapshot of the repository before
//! it runs, and an exit hook compares it with the state afterwards and says, in plain words,
//! what actually happened (or why nothing did) and what to do next. All wording lives in this
//! file. Narration goes to stderr only, so stdout stays exactly as before.
//!
//! Levels: `quiet` (git-identical, nothing extra), `normal` (one-line notes, conflict reports,
//! warnings), `explain` (+ what it means), `teach` (+ the refs and index changed underneath).
//! Chosen by `rgit --plain|--explain|--teach <cmd>`, `RGIT_VERBOSITY`, or `rgit.verbosity`;
//! the default is `normal`, but only when stderr is a terminal, so scripts and parity tests
//! that pipe rgit's output see no change.

pub mod narrate;

use std::collections::BTreeSet;
use std::io::IsTerminal;
use std::sync::{Mutex, OnceLock};

use crate::core::commit::commit_parents;
use crate::ui::narrate::*;
use crate::objects::{RepoSnapshot, Verbosity};

static LEVEL: OnceLock<Verbosity> = OnceLock::new();
static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static OUTCOME: Mutex<Option<bool>> = Mutex::new(None);

/// The command being narrated: its name, arguments and the state before it ran.
struct Pending {
    command: String,
    args: Vec<String>,
    before: Option<RepoSnapshot>,
    cwd: std::path::PathBuf,
}

/// Parses a verbosity name (`quiet`/`plain`, `normal`, `explain`/`verbose`, `teach`).
fn parse_level(value: &str) -> Option<Verbosity> {
    match value.trim().to_ascii_lowercase().as_str() {
        "quiet" | "plain" | "off" | "0" => Some(Verbosity::Quiet),
        "normal" | "1" => Some(Verbosity::Normal),
        "explain" | "verbose" | "2" => Some(Verbosity::Explain),
        "teach" | "3" => Some(Verbosity::Teach),
        _ => None,
    }
}

/// Decides the narration level once: a global flag wins, then `RGIT_VERBOSITY`, then
/// `rgit.verbosity`; without any of them, `normal` when stderr is a terminal (and this is not
/// an rgit started by rgit), `quiet` otherwise.
pub fn init(flag: Option<Verbosity>) {
    let explicit = flag
        .or_else(|| std::env::var("RGIT_VERBOSITY").ok().and_then(|v| parse_level(&v)))
        .or_else(|| crate::core::config::get_config_value("rgit.verbosity").ok().flatten().and_then(|v| parse_level(&v)));
    let level = explicit.unwrap_or_else(|| {
        if std::io::stderr().is_terminal() && std::env::var_os("RGIT_NESTED").is_none() {
            Verbosity::Normal
        } else {
            Verbosity::Quiet
        }
    });
    let _ = LEVEL.set(level);
}

/// The narration level (quiet until `init` ran).
pub fn level() -> Verbosity {
    *LEVEL.get().unwrap_or(&Verbosity::Quiet)
}

fn colors() -> bool {
    std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none() && (cfg!(not(windows)) || std::env::var_os("WT_SESSION").is_some() || std::env::var_os("TERM").is_some())
}

fn paint(code: &str, text: &str) -> String {
    if colors() { format!("\x1b[{}m{}\x1b[0m", code, text) } else { text.to_string() }
}

/// A one-line note about what happened (normal level).
pub fn note(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("{} {}", paint("36", "rgit:"), text);
    }
}

/// An indented continuation line (normal level).
fn line(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("      {}", text);
    }
}

/// Extra explanation, shown from the explain level up.
pub fn detail(text: &str) {
    if level() >= Verbosity::Explain {
        eprintln!("      {}", paint("2", text));
    }
}

/// A warning that might prevent a mistake (yellow, normal level).
pub fn warn(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("{} {}", paint("33", "rgit: warning:"), text);
    }
}

/// Something that loses data or rewrites shared history (red, normal level).
pub fn danger(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("{} {}", paint("31", "rgit:"), text);
    }
}

/// The exact next command(s) to run (normal level).
fn next(text: &str) {
    if level() >= Verbosity::Normal {
        eprintln!("{} {}", paint("32", "  next:"), text);
    }
}

/// Plumbing-level detail, shown only at the teach level.
fn teach(text: &str) {
    if level() >= Verbosity::Teach {
        eprintln!("{} {}", paint("35", "  under the hood:"), text);
    }
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

fn list(paths: &[String], max: usize) -> String {
    let mut out = paths.iter().take(max).cloned().collect::<Vec<_>>().join(", ");
    if paths.len() > max {
        out.push_str(&format!(" and {} more", paths.len() - max));
    }
    out
}

/// Commands whose effects rgit narrates, and whether their snapshot needs the working tree.
fn narrated(command: &str) -> Option<bool> {
    match command {
        "add" | "stage" | "commit" | "reset" | "restore" | "checkout" | "switch" | "stash" | "rm" | "clean" | "diff" => Some(true),
        "merge" | "pull" | "fetch" | "push" | "rebase" | "cherry-pick" | "revert" | "tag" | "branch" | "status" | "clone" | "log" => Some(false),
        _ => None,
    }
}

fn split_args() -> (String, Vec<String>) {
    let mut args = std::env::args().skip(1).skip_while(|a| a.starts_with('-'));
    let command = args.next().unwrap_or_default();
    (if command == "stage" { "add".into() } else { command }, args.collect())
}

/// Positional (non-option) arguments, skipping the values of options that take one.
fn positionals(args: &[String], value_flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    let mut after_dashdash = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if after_dashdash {
            out.push(a.clone());
        } else if a == "--" {
            after_dashdash = true;
        } else if a.starts_with('-') && a.len() > 1 {
            if value_flags.contains(&a.as_str()) {
                skip = true;
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

fn has(args: &[String], flags: &[&str]) -> bool {
    args.iter().any(|a| flags.contains(&a.as_str()) || flags.iter().any(|f| f.starts_with("--") && a.starts_with(&format!("{}=", f))))
}

unsafe extern "C" {
    fn atexit(callback: extern "C" fn()) -> std::os::raw::c_int;
}

extern "C" fn at_exit() {
    let _ = std::panic::catch_unwind(finish);
}

/// Called once the command returned normally (true) or with an error (false). Commands that
/// stop early go through `ui::exit`, which records the outcome from the exit code.
/// Whether git-style conflict hints should be left out because the narration will explain the
/// same stop in its own words (narrating at normal level or above). Piped output keeps them.
pub fn narration_replaces_hints() -> bool {
    level() >= Verbosity::Normal && PENDING.lock().map(|p| p.is_some()).unwrap_or(false)
}

/// Ends the process like `std::process::exit`, recording the outcome first so the exit hook
/// knows whether the command succeeded. Every early exit in rgit goes through here.
pub fn exit(code: i32) -> ! {
    set_outcome(code == 0);
    // Report now instead of relying on the `atexit` hook: on Windows `std::process::exit` is
    // `ExitProcess`, which skips CRT atexit handlers. `finish` takes the pending report, so the
    // hook finds nothing left to do on platforms where it does run.
    let _ = std::panic::catch_unwind(finish);
    std::process::exit(code)
}

pub fn set_outcome(ok: bool) {
    if let Ok(mut o) = OUTCOME.lock() {
        *o = Some(ok);
    }
}

/// Starts narrating the current command: takes the "before" snapshot, prints warnings that
/// must come before the command acts, and registers the exit hook that reports afterwards.
pub fn begin() {
    if level() == Verbosity::Quiet || !crate::core::repo::exists() && split_args().0 != "clone" {
        return;
    }
    let (command, args) = split_args();
    let Some(with_worktree) = narrated(&command) else { return };
    // rgit processes started by this one (submodules, hooks, for-each-repo) stay quiet.
    unsafe { std::env::set_var("RGIT_NESTED", "1") };
    let before = if command == "clone" { None } else { Some(take_snapshot(with_worktree)) };
    if let Some(b) = &before {
        before_command(&command, &args, b);
    }
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(Pending { command, args, before, cwd: std::env::current_dir().unwrap_or_default() });
    }
    unsafe {
        atexit(at_exit);
    }
}

fn finish() {
    let Some(p) = PENDING.lock().ok().and_then(|mut g| g.take()) else { return };
    let _ = std::env::set_current_dir(&p.cwd);
    let outcome = OUTCOME.lock().ok().and_then(|o| *o);
    if p.command == "clone" {
        if outcome == Some(true) {
            report_clone(&p.args);
        }
        return;
    }
    let Some(before) = p.before else { return };
    let after = take_snapshot(narrated(&p.command).unwrap_or(false));
    report(&p.command, &p.args, &before, &after, outcome);
    teach_changes(&before, &after);
}

// ----------------------------------------------------------------------------------------
// Before the command runs
// ----------------------------------------------------------------------------------------

fn before_command(command: &str, args: &[String], b: &RepoSnapshot) {
    match command {
        "push" => {
            let Some(branch) = &b.head_branch else { return };
            let pos = positionals(args, &[]);
            let remote = pos.first().cloned().unwrap_or_else(|| crate::core::config::get_config_value(&format!("branch.{}.remote", branch)).ok().flatten().unwrap_or_else(|| "origin".into()));
            let tracking = format!("refs/remotes/{}/{}", remote, branch);
            if has(args, &["-f", "--force"]) {
                if let (Some(theirs), Some(ours)) = (b.refs.get(&tracking), &b.head) {
                    if !is_ancestor(theirs, ours) {
                        let lost = commits_between(Some(ours), theirs);
                        danger(&format!("force-pushing over {} on {}/{} that your branch doesn't have; they'll be gone from the remote.", plural(lost, "commit", "commits"), remote, branch));
                        detail("If someone else pushed them, they lose their work. Safer: `rgit pull` first, then push normally.");
                    }
                }
            }
            if ["main", "master"].contains(&branch.as_str()) {
                detail(&format!("You're pushing straight to '{}', the default branch.", branch));
            }
        }
        "diff" if positionals(args, &[]).is_empty() => {
            if has(args, &["--cached", "--staged"]) {
                detail("Comparing the index with HEAD: these are your STAGED changes (what `rgit commit` would record).");
            } else {
                detail("Comparing the working tree with the index: these are your UNSTAGED changes (`--cached` shows the staged ones).");
            }
        }
        "reset" if has(args, &["--hard"]) => {
            let changed = b.worktree_changes.as_ref().map(|c| c.len()).unwrap_or(0);
            if changed + b.staged.len() > 0 {
                detail("--hard overwrites the index AND your files: uncommitted work will be lost.");
            }
        }
        _ => {}
    }
}

// ----------------------------------------------------------------------------------------
// After the command: what actually happened
// ----------------------------------------------------------------------------------------

fn report(command: &str, args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn teach_changes(b: &RepoSnapshot, a: &RepoSnapshot) {
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
    crate::references::for_each_ref::engine::branch_upstream(branch).or_else(|| {
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
            let short_up = crate::references::for_each_ref::engine::shorten_unambiguous_ref(&up);
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

fn report_add(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn report_commit(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
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

fn merge_target_name(args: &[String]) -> Option<String> {
    positionals(args, &["-m", "-F", "-s", "-X", "--strategy", "--strategy-option", "--file", "--message"]).into_iter().next()
}

fn report_merge(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>, quiet_noop: bool) {
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
    let short_up = crate::references::for_each_ref::engine::shorten_unambiguous_ref(&up);
    if behind > 0 {
        note(&format!("'{}' is now {} behind {}{}.", branch, plural(behind, "commit", "commits"), short_up, if ahead > 0 { format!(" and {} ahead (diverged)", ahead) } else { String::new() }));
        next(&format!("`rgit pull` (or `rgit merge {}`) to catch up.", short_up));
    }
}

fn report_fetch(b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn report_pull(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
    let changes = remote_changes(b, a);
    if changes.is_empty() && a.head == b.head {
        if ok == Some(true) {
            let up = a.head_branch.as_ref().and_then(|br| upstream_of(br)).map(|u| crate::references::for_each_ref::engine::shorten_unambiguous_ref(&u)).unwrap_or_else(|| "the remote".into());
            note(&format!("Already up to date: {} has nothing new for you.", up));
        }
        return;
    }
    if !changes.is_empty() {
        note(&format!("Fetched: {}.", describe_fetched(&changes)));
    }
    report_merge(args, b, a, ok, true);
}

fn report_push(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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
                (_, Some(n)) => {
                    let sent = commits_between(old.as_deref(), n);
                    note(&format!("Sent {} to {} ({}{}).", plural(sent, "commit", "commits"), name, old.as_deref().map(|o| format!("{}..", short(o))).unwrap_or_default(), short(n)));
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

fn report_rebase(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn report_pick(command: &str, args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
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

fn report_conflicts(a: &RepoSnapshot, b: &RepoSnapshot) {
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

/// `log` on a branch with no commits: git only says "does not have any commits yet".
fn report_log(args: &[String], a: &RepoSnapshot) {
    if a.head.is_some() || !positionals(args, &[]).is_empty() || has(args, &["--all"]) {
        return;
    }
    note("There is no history yet: nothing has been committed on this branch.");
    let others: Vec<String> = a.refs.keys().filter_map(|r| r.strip_prefix("refs/heads/")).map(String::from).collect();
    if !others.is_empty() {
        next(&format!("other branches have commits: {} (`rgit log {}` or `rgit log --all`).", list(&others, 4), others[0]));
    }
    if a.staged.is_empty() {
        next("`rgit add <files>`, then `rgit commit -m \"<message>\"` makes the first commit.");
    } else {
        next(&format!("{} staged; `rgit commit -m \"<message>\"` makes the first commit.", plural(a.staged.len(), "file is", "files are")));
    }
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

/// "2 of 3 conflicted files resolved" while an operation that stopped on conflicts is in progress.
fn report_status_progress(a: &RepoSnapshot) {
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

fn report_reset(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn report_restore(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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
        let compare = upstream_of(&branch).and_then(|u| a.refs.get(&u).map(|x| (crate::references::for_each_ref::engine::shorten_unambiguous_ref(&u), x.clone())))
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

fn report_switch(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn report_checkout(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn report_stash(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn report_rm(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
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

fn report_clean(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot, ok: Option<bool>) {
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

fn report_tag(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
    if has(args, &["-l", "--list", "-d", "--delete", "-v"]) {
        return;
    }
    let pos = positionals(args, &["-m", "-F", "--message", "--file"]);
    let Some(name) = pos.first() else { return };
    let refname = format!("refs/tags/{}", name);
    match (b.refs.get(&refname), a.refs.get(&refname)) {
        (Some(old), Some(new)) if old == new => {
            let target = crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", name)).unwrap_or_else(|_| old.clone());
            note(&format!("Tag '{}' already exists (points to {}); `rgit tag -f {}` moves it.", name, short(&target), name));
        }
        (old, Some(new)) => {
            let annotated = crate::core::object_store::read_object(new).map(|(k, _)| k == "tag").unwrap_or(false);
            let target = crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", name)).unwrap_or_else(|_| new.clone());
            let (kind, gloss) = if annotated { ("annotated tag", "it records author, date and message") } else { ("lightweight tag", "just a name for a commit") };
            let verb = if old.is_some() { "Moved" } else { "Created" };
            note(&format!("{} {} '{}' on {} ({}).", verb, kind, name, short(&target), gloss));
            detail(&format!("Tags aren't pushed by default: `rgit push origin {}` shares it.", name));
        }
        _ => {}
    }
}

fn report_branch(args: &[String], b: &RepoSnapshot, a: &RepoSnapshot) {
    for (name, hash) in &b.refs {
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            if !a.refs.contains_key(name) && !a.refs.keys().any(|n| n.starts_with("refs/heads/") && a.refs[n] == *hash && b.refs.get(n).is_none()) {
                note(&format!("Undo: `rgit branch {} {}` brings it back (its commits stay recoverable until the reflog expires).", branch, short(hash)));
            }
        }
    }
    let renaming = has(args, &["-m", "-M", "--move", "-c", "-C", "--copy"]);
    for (name, hash) in &a.refs {
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            if !b.refs.contains_key(name) && !renaming {
                note(&format!("Created '{}' at {}; you're still on '{}' (`rgit switch {}` to move there).", branch, short(hash), a.head_branch.clone().unwrap_or_else(|| "a detached HEAD".into()), branch));
            }
        }
    }
}

fn report_diff(args: &[String], b: &RepoSnapshot, ok: Option<bool>) {
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

fn report_clone(args: &[String]) {
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
