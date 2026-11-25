//! The checks behind `rgit doctor`: objects, refs, identity, config, hooks, lock files,
//! worktrees, huge untracked files and operations left in progress. Each returns findings with
//! the command that fixes them; `apply` carries out the fixes `--fix` may make by itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::core::config::{get_config_value, read_all_config_entries};
use crate::storage::doctor::types::{AutoFix, Finding, Severity};

/// A lock file older than this was left behind by a command that died.
pub const STALE_LOCK: Duration = Duration::from_secs(10 * 60);
/// An operation in progress for longer than this was probably forgotten.
pub const FORGOTTEN_OPERATION: Duration = Duration::from_secs(24 * 3600);

/// `500K`, `50M`, `1G` or a plain number of bytes.
// Used by `rgit doctor --large`.
pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim().to_ascii_uppercase();
    let (digits, factor) = match t.chars().last()? {
        'K' => (&t[..t.len() - 1], 1024),
        'M' => (&t[..t.len() - 1], 1024 * 1024),
        'G' => (&t[..t.len() - 1], 1024 * 1024 * 1024),
        _ => (t.as_str(), 1),
    };
    digits.parse::<u64>().ok().map(|n| n * factor)
}

/// `240 MB`, `512 KB`, `12 bytes`.
// Used by `check_large_files`.
fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1024 * 1024 * 1024 => format!("{:.1} GB", b as f64 / (1024.0 * 1024.0 * 1024.0)),
        b if b >= 1024 * 1024 => format!("{} MB", b / (1024 * 1024)),
        b if b >= 1024 => format!("{} KB", b / 1024),
        b => format!("{} bytes", b),
    }
}

/// How long ago a file was last modified (zero when unknown or in the future).
// Used by `check_locks` and `check_operations`.
fn age(path: &Path) -> Duration {
    fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).unwrap_or_default()
}

/// `5 minutes` / `3 hours` / `2 days`.
// Used by `check_locks` and `check_operations`.
fn span(d: Duration) -> String {
    let s = d.as_secs();
    let unit = |n: u64, what: &str| format!("{} {}{}", n, what, if n == 1 { "" } else { "s" });
    match s {
        0..=119 => unit(s, "second"),
        120..=3599 => unit(s / 60, "minute"),
        3600..=172_799 => unit(s / 3600, "hour"),
        _ => unit(s / 86_400, "day"),
    }
}

/// Object integrity and connectivity (`fsck`): corrupt and missing objects are errors, other
/// fsck complaints warnings; dangling objects are normal and not reported.
// Used by the `doctor` command (skipped with `--quick`).
pub fn check_objects() -> Result<Vec<Finding>> {
    let report = crate::storage::fsck::engine::run_fsck_checks(false, true)?;
    Ok(report
        .issues
        .into_iter()
        .map(|issue| {
            let error = issue.severity == crate::storage::fsck::types::FsckSeverity::Error;
            let fix = error.then(|| "objects can't be rebuilt locally: `rgit fetch` from a remote that has them, or clone again".to_string());
            Finding::new(if error { Severity::Error } else { Severity::Warning }, format!("objects: {}", issue.message.trim_start_matches("error: ").trim_start_matches("warning: ")), fix)
        })
        .collect())
}

/// The ref database (`refs verify`).
// Used by the `doctor` command.
pub fn check_refs() -> Vec<Finding> {
    let (_, problems) = crate::references::refs_verify::verify_refs(false);
    problems
        .into_iter()
        .map(|(error, message)| Finding::new(if error { Severity::Error } else { Severity::Warning }, format!("refs: {}", message.trim_start_matches("error: ").trim_start_matches("warning: ")), Some("`rgit refs verify` shows the details".to_string())))
        .collect()
}

/// `user.name` / `user.email` missing (and not set through `GIT_AUTHOR_*` / `GIT_COMMITTER_*`):
/// commits silently get rgit's placeholder identity.
// Used by the `doctor` command.
pub fn check_identity() -> Result<Vec<Finding>> {
    let mut out = Vec::new();
    for (key, var, placeholder, example) in [("user.name", "NAME", "rgit", "\"Your Name\""), ("user.email", "EMAIL", "rgit@example.com", "you@example.com")] {
        let configured = get_config_value(key)?.is_some_and(|v| !v.trim().is_empty());
        let from_env = ["AUTHOR", "COMMITTER"].iter().all(|role| std::env::var(format!("GIT_{}_{}", role, var)).is_ok_and(|v| !v.trim().is_empty()));
        if !configured && !from_env {
            out.push(Finding::new(Severity::Warning, format!("{} is not set: commits will say '{}'", key, placeholder), Some(format!("rgit config --global {} {}", key, example))));
        }
    }
    Ok(out)
}

/// Lines `rgit config` can't read in one config file: a broken `[section]` header, or a key
/// before any section.
// Used by `check_config`.
fn malformed_lines(path: &Path, label: &str) -> Vec<Finding> {
    let Ok(content) = fs::read_to_string(path) else { return Vec::new() };
    let mut out = Vec::new();
    let mut in_section = false;
    for (i, raw) in content.lines().enumerate() {
        let line = raw.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let problem = if line.starts_with('[') {
            in_section = line.contains(']');
            (!in_section).then_some("a [section] header without its closing ]")
        } else if !in_section {
            Some("a setting outside any [section]")
        } else {
            None
        };
        if let Some(problem) = problem {
            out.push(Finding::new(Severity::Warning, format!("config: {} line {} is {}, so it is ignored: {}", label, i + 1, problem, line), Some(format!("edit {} (or `rgit config --unset` the setting and set it again)", label))));
        }
    }
    out
}

/// Config that points nowhere: malformed lines, branches tracking a remote that doesn't exist
/// or a remote branch that is gone, local-path remote URLs that don't exist, and
/// `core.hooksPath` / `core.excludesFile` naming missing paths.
// Used by the `doctor` command.
pub fn check_config() -> Result<Vec<Finding>> {
    let mut out = malformed_lines(&crate::core::repo::path(".git/config"), ".git/config");
    if let Some(global) = crate::core::config::global_config_path() {
        out.extend(malformed_lines(&global, &global.to_string_lossy().replace('\\', "/")));
    }
    let entries = read_all_config_entries();
    let remotes = crate::remote::remote::engine::list_remote_names().unwrap_or_default();
    for (key, value) in &entries {
        let Some(rest) = key.strip_prefix("branch.") else { continue };
        let Some(branch) = rest.strip_suffix(".remote") else { continue };
        if value == "." || remotes.contains(value) {
            if let Some(merge) = get_config_value(&format!("branch.{}.merge", branch))?
                && value != "."
                && crate::core::refs::read_ref(&format!("refs/heads/{}", branch))?.is_some()
            {
                let short = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
                if crate::core::refs::read_ref(&format!("refs/remotes/{}/{}", value, short))?.is_none() {
                    out.push(Finding::new(
                        Severity::Warning,
                        format!("branch '{}' tracks {}/{}, which is gone (deleted on the remote, or never fetched)", branch, value, short),
                        Some(format!("rgit fetch {}; if it's still gone: rgit branch --unset-upstream {}", value, branch)),
                    ));
                }
            }
            continue;
        }
        out.push(Finding::new(Severity::Warning, format!("branch '{}' tracks remote '{}', which doesn't exist", branch, value), Some(format!("rgit branch --unset-upstream {}", branch))));
    }
    for remote in &remotes {
        if let Ok((Some(url), _)) = crate::remote::remote::engine::get_remote_urls(remote) {
            // `host:path` / `user@host:path` (but not a `C:` drive) is ssh, not a local path.
            let scp_like = url.find(':').is_some_and(|i| i > 1 && !url[..i].contains('/'));
            let local = !url.contains("://") && !scp_like;
            if local && !Path::new(&url).exists() {
                out.push(Finding::new(Severity::Warning, format!("remote '{}' points to {}, which doesn't exist", remote, url), Some(format!("rgit remote set-url {} <new-url>", remote))));
            }
        }
    }
    for (key, what) in [("core.hookspath", "hooks directory"), ("core.excludesfile", "global ignore file")] {
        if let Some(value) = get_config_value(key)?.filter(|v| !v.is_empty()) {
            let path = match value.strip_prefix("~/") {
                Some(rest) => std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join(rest)).unwrap_or_else(|| PathBuf::from(&value)),
                None => PathBuf::from(&value),
            };
            if !path.exists() {
                let name = if key == "core.hookspath" { "core.hooksPath" } else { "core.excludesFile" };
                out.push(Finding::new(Severity::Warning, format!("{} is {}, which doesn't exist (no {} is used)", name, value, what), Some(format!("rgit config --unset {} (or create it)", name))));
            }
        }
    }
    Ok(out)
}

/// Hooks that never run because they are not executable (Unix only: Windows has no such bit).
// Used by the `doctor` command.
pub fn check_hooks() -> Vec<Finding> {
    let dir = match get_config_value("core.hookspath").ok().flatten().filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => crate::core::repo::path(".git/hooks"),
    };
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else { return out };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_none_or(|x| x != "sample")).collect();
    paths.sort();
    for path in paths {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(&path).map(|m| m.permissions().mode() & 0o111 == 0).unwrap_or(false) {
                let shown = path.to_string_lossy().replace('\\', "/");
                out.push(Finding {
                    severity: Severity::Warning,
                    message: format!("hook {} is not executable, so it never runs", shown),
                    fix: Some(format!("chmod +x {} (or `rgit doctor --fix`)", shown)),
                    auto: Some(AutoFix::MakeExecutable(path.clone())),
                });
            }
        }
        #[cfg(not(unix))]
        let _ = path;
    }
    out
}

/// Every `*.lock` file under `dir`, recursively (`objects/` is skipped: no locks live there).
// Used by `check_locks`.
fn lock_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "objects" {
                lock_files(&path, out);
            }
        } else if path.extension().is_some_and(|x| x == "lock") {
            out.push(path);
        }
    }
}

/// `*.lock` files in `.git`: older than `STALE_LOCK` they were left by a command that died and
/// make every later write fail (an error, removable by `--fix`); a recent one may belong to a
/// command still running (a warning).
// Used by the `doctor` command.
pub fn check_locks() -> Vec<Finding> {
    let mut locks = Vec::new();
    lock_files(&crate::core::repo::path(".git"), &mut locks);
    locks.sort();
    locks
        .into_iter()
        .map(|path| {
            let shown = path.to_string_lossy().replace('\\', "/");
            let shown = shown.find(".git/").map(|i| shown[i..].to_string()).unwrap_or(shown);
            let old = age(&path);
            if old >= STALE_LOCK {
                Finding {
                    severity: Severity::Error,
                    message: format!("{} is {} old: a command that crashed left it, and writes to it will fail", shown, span(old)),
                    fix: Some("rgit doctor --fix (removes stale locks)".to_string()),
                    auto: Some(AutoFix::RemoveLock(path)),
                }
            } else {
                Finding::new(Severity::Warning, format!("{} is recent (under {} minutes old): a git or rgit command may still be running", shown, STALE_LOCK.as_secs() / 60), Some(format!("wait for it; if nothing is running, delete {}", shown)))
            }
        })
        .collect()
}

/// Linked worktrees whose directory is gone (`worktree prune` removes their records).
// Used by the `doctor` command.
pub fn check_worktrees() -> Result<Vec<Finding>> {
    Ok(crate::workspace::worktree::engine::list_worktrees()?
        .into_iter()
        .filter_map(|wt| {
            let reason = wt.prunable?;
            Some(Finding {
                severity: Severity::Warning,
                message: format!("worktree '{}' is broken: {}", wt.id.unwrap_or_default(), reason),
                fix: Some("rgit worktree prune (or `rgit doctor --fix`)".to_string()),
                auto: Some(AutoFix::PruneWorktrees),
            })
        })
        .collect())
}

/// Untracked, not ignored files of at least `threshold` bytes: `rgit add .` would commit them.
// Used by the `doctor` command.
pub fn check_large_files(threshold: u64) -> Result<Vec<Finding>> {
    let tracked: std::collections::BTreeSet<String> = crate::core::index::read_index().unwrap_or_default().into_iter().map(|e| e.path).collect();
    let mut out = Vec::new();
    for path in crate::workspace::state_files::collect_untracked_files(&tracked, false)? {
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size >= threshold {
            let target = match path.rsplit_once('/') {
                Some((dir, _)) => format!("{}/", dir),
                None => path.clone(),
            };
            out.push(Finding::new(Severity::Warning, format!("untracked {} is {}: `rgit add .` would commit it", path, human(size)), Some(format!("rgit ignore {}", target))));
        }
    }
    Ok(out)
}

/// A merge, rebase, cherry-pick, revert, bisect or am in progress for over a day.
// Used by the `doctor` command.
pub fn check_operations() -> Vec<Finding> {
    let state = |p: &str| crate::core::repo::path(p);
    let op = crate::ui::narrate::operation_in_progress().or_else(|| state(".git/rebase-apply").exists().then(|| "am".to_string()));
    let Some(op) = op else { return Vec::new() };
    let marker = match op.as_str() {
        "rebase" => state(crate::merge::rebase::engine::REBASE_ORIG_HEAD_PATH),
        "merge" => state(".git/MERGE_HEAD"),
        "cherry-pick" => state(".git/CHERRY_PICK_HEAD"),
        "revert" => state(".git/REVERT_HEAD"),
        "bisect" => state(crate::history::bisect::engine::BISECT_START_PATH),
        "am" => state(".git/rebase-apply"),
        _ => return Vec::new(),
    };
    let old = age(&marker);
    if old < FORGOTTEN_OPERATION {
        return Vec::new();
    }
    let fix = if op == "bisect" { "rgit bisect reset".to_string() } else { format!("rgit {} --continue, or rgit {} --abort", op, op) };
    vec![Finding::new(Severity::Warning, format!("a {} has been in progress for {}", op, span(old)), Some(fix))]
}

/// Carries out one automatic fix; returns what was done.
// Used by `rgit doctor --fix`.
pub fn apply(fix: &AutoFix) -> Result<String> {
    match fix {
        AutoFix::RemoveLock(path) => {
            fs::remove_file(path)?;
            let shown = path.to_string_lossy().replace('\\', "/");
            Ok(format!("removed {}", shown.find(".git/").map(|i| shown[i..].to_string()).unwrap_or(shown)))
        }
        AutoFix::PruneWorktrees => {
            let pruned = crate::workspace::worktree::engine::prune_worktrees(false)?;
            Ok(format!("pruned worktree{} {}", if pruned.len() == 1 { "" } else { "s" }, pruned.iter().map(|(id, _)| format!("'{}'", id)).collect::<Vec<_>>().join(", ")))
        }
        AutoFix::MakeExecutable(path) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(path)?.permissions();
                perms.set_mode(perms.mode() | 0o111);
                fs::set_permissions(path, perms)?;
            }
            Ok(format!("made {} executable", path.to_string_lossy().replace('\\', "/")))
        }
    }
}
