use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::helpers::config::author_identity;
use crate::objects::ReflogEntry;

/// The hash a reflog line uses for "no commit": the old value of a ref that was just created,
/// or the new value of one that was deleted.
pub const NULL_HASH: &str = "0000000000000000000000000000000000000000";

/// Longest message stored in a reflog line; anything beyond is cut off.
const MAX_MESSAGE_LEN: usize = 200;

/// Directory holding all reflogs; `HEAD` and `refs/heads/<b>` live under it at the same
/// relative paths as the refs themselves, which is also git's layout.
const LOGS_DIR: &str = ".git/logs";

/// The message attached to reflog entries written by the current command (a process runs
/// exactly one command, so one process-wide value is enough).
static REFLOG_ACTION: Mutex<String> = Mutex::new(String::new());

/// Sets the message recorded with every reflog entry the current command writes. `cli.rs`
/// sets it to the command line as typed; a command can call this again to describe what it
/// is doing more precisely (e.g. `commit: fix typo`). Control characters become spaces and
/// the text is cut to `MAX_MESSAGE_LEN`, so an entry always stays on one line.
// Used by `handle_commands` and the `commit` command.
pub fn set_reflog_action(message: &str) {
    let cleaned: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let cleaned: String = cleaned.trim().chars().take(MAX_MESSAGE_LEN).collect();

    if let Ok(mut action) = REFLOG_ACTION.lock() {
        *action = cleaned;
    }
}

/// Returns the message to record for the current command.
// Used by `record_ref_update` and `record_head_move`.
fn current_reflog_action() -> String {
    match REFLOG_ACTION.lock() {
        Ok(action) if !action.is_empty() => action.clone(),
        _ => "update".to_string(),
    }
}

/// Returns the path of the reflog file for a ref (`HEAD`, `refs/heads/main`, ...).
// Used by every function in this file that reads or writes a reflog.
fn reflog_file(ref_name: &str) -> String {
    format!("{}/{}", LOGS_DIR, ref_name)
}

/// Says whether updates to `ref_name` get a reflog: `HEAD`, branches and remote-tracking
/// branches do, tags and anything else do not (same as git's default).
// Used by `record_ref_update`.
fn is_logged_ref(ref_name: &str) -> bool {
    ref_name == "HEAD" || ref_name.starts_with("refs/heads/") || ref_name.starts_with("refs/remotes/")
}

/// Appends one entry to a ref's reflog, stamped with the configured identity and the current
/// time.
// Used by `record_ref_update` and `record_head_move`.
fn append_reflog_entry(ref_name: &str, old_hash: Option<&str>, new_hash: Option<&str>) -> Result<()> {
    let identity = author_identity().unwrap_or_else(|_| "rgit <rgit@example.com>".to_string());
    let entry = ReflogEntry {
        old_hash: old_hash.unwrap_or(NULL_HASH).to_string(),
        new_hash: new_hash.unwrap_or(NULL_HASH).to_string(),
        identity,
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        tz: "+0000".to_string(),
        message: current_reflog_action(),
    };

    let path = reflog_file(ref_name);
    if let Some(parent) = Path::new(&path).parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open {}", path))?;
    writeln!(file, "{}", entry.to_line())?;
    Ok(())
}

/// Records that `ref_name` moved from `old_hash` to `new_hash`. Does nothing for refs that
/// aren't logged or when the value didn't change. Best effort: a failure to write the log
/// must never fail the operation that moved the ref, so errors are swallowed.
// Used by `write_ref` and `set_head_detached`.
pub fn record_ref_update(ref_name: &str, old_hash: Option<&str>, new_hash: Option<&str>) {
    if !is_logged_ref(ref_name) || old_hash == new_hash {
        return;
    }
    let _ = append_reflog_entry(ref_name, old_hash, new_hash);
}

/// Records a change of what HEAD points at in the `HEAD` reflog even when the commit stayed
/// the same (switching between two branches at one commit is still a move). Best effort, like
/// `record_ref_update`.
// Used by `set_head` and `set_head_detached`.
pub fn record_head_move(old_hash: Option<&str>, new_hash: Option<&str>) {
    if old_hash.is_none() && new_hash.is_none() {
        return;
    }
    let _ = append_reflog_entry("HEAD", old_hash, new_hash);
}

/// Reads a ref's reflog, newest entry first (so index `n` is `<ref>@{n}`). A missing log is
/// an empty one; lines that don't parse are skipped.
// Used by the `reflog` command and `resolve_reflog_revision`.
pub fn read_reflog(ref_name: &str) -> Result<Vec<ReflogEntry>> {
    let path = reflog_file(ref_name);
    if !Path::new(&path).exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path))?;
    let mut entries: Vec<ReflogEntry> = content.lines().filter_map(ReflogEntry::parse).collect();
    entries.reverse();
    Ok(entries)
}

/// Rewrites a ref's reflog from `entries` (newest first, as `read_reflog` returns them). An
/// empty list removes the log file.
// Used by the `reflog` command (`delete` and `expire`).
pub fn write_reflog(ref_name: &str, entries: &[ReflogEntry]) -> Result<()> {
    let path = reflog_file(ref_name);
    if entries.is_empty() {
        delete_reflog(ref_name);
        return Ok(());
    }

    let mut content = String::new();
    for entry in entries.iter().rev() {
        content.push_str(&entry.to_line());
        content.push('\n');
    }
    fs::write(&path, content).with_context(|| format!("Failed to write {}", path))?;
    Ok(())
}

/// Removes empty directories left behind under `.git/logs` after a reflog file went away,
/// so deleting `feature/login`'s log doesn't leave an empty `feature/` folder.
// Used by `delete_reflog` and `rename_reflog`.
fn prune_empty_log_dirs(file_path: &str) {
    let root = Path::new(LOGS_DIR);
    let mut parent = Path::new(file_path).parent();
    while let Some(dir) = parent {
        if dir == root || !dir.starts_with(root) {
            break;
        }
        let is_empty = fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(false);
        if !is_empty || fs::remove_dir(dir).is_err() {
            break;
        }
        parent = dir.parent();
    }
}

/// Deletes a ref's reflog (git removes a branch's log together with the branch). Best effort.
// Used by `delete_branch` and `write_reflog`.
pub fn delete_reflog(ref_name: &str) {
    let path = reflog_file(ref_name);
    if Path::new(&path).exists() && fs::remove_file(&path).is_ok() {
        prune_empty_log_dirs(&path);
    }
}

/// Moves a ref's reflog to follow the ref when a branch is renamed. Best effort.
// Used by `rename_branch`.
pub fn rename_reflog(old_ref: &str, new_ref: &str) {
    let old_path = reflog_file(old_ref);
    let new_path = reflog_file(new_ref);
    if !Path::new(&old_path).exists() {
        return;
    }
    if let Some(parent) = Path::new(&new_path).parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if fs::rename(&old_path, &new_path).is_ok() {
        prune_empty_log_dirs(&old_path);
    }
}

/// Collects the name of every ref that has a reflog (`HEAD`, `refs/heads/main`, ...), sorted.
// Used by the `reflog` command (`expire --all`).
pub fn list_reflog_refs() -> Result<Vec<String>> {
    fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
        for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let full = if prefix.is_empty() { name } else { format!("{}/{}", prefix, name) };
            if entry.metadata()?.is_dir() {
                walk(&entry.path(), &full, out)?;
            } else {
                out.push(full);
            }
        }
        Ok(())
    }

    let root = Path::new(LOGS_DIR);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut refs = Vec::new();
    walk(root, "", &mut refs)?;
    refs.sort();
    Ok(refs)
}

/// Turns what the user typed (`HEAD`, `main`, `origin/main`, `refs/heads/main`) into the ref
/// whose reflog to read: `HEAD` and full `refs/...` names are used as given, a short name is
/// looked up as a branch and then as a remote-tracking branch, preferring one that has a log.
// Used by the `reflog` command and `resolve_reflog_revision`.
pub fn resolve_reflog_ref_name(name: &str) -> String {
    if name == "HEAD" || name.starts_with("refs/") {
        return name.to_string();
    }
    for prefix in ["refs/heads/", "refs/remotes/"] {
        let candidate = format!("{}{}", prefix, name);
        if Path::new(&reflog_file(&candidate)).exists() {
            return candidate;
        }
    }
    format!("refs/heads/{}", name)
}

/// Splits a `<ref>@{<n>}` spec into the ref name (`HEAD` when omitted, as in `@{1}`) and the
/// entry number. Returns `None` if `spec` isn't of that form.
// Used by `resolve_reflog_revision` and the `reflog` command (`delete`).
pub fn parse_reflog_spec(spec: &str) -> Option<(String, usize)> {
    let inner = spec.strip_suffix('}')?;
    let at = inner.rfind("@{")?;
    let index: usize = inner[at + 2..].parse().ok()?;
    let name = &inner[..at];
    Some((if name.is_empty() { "HEAD".to_string() } else { name.to_string() }, index))
}

/// Resolves `HEAD@{n}` / `main@{n}` / `@{n}` to the commit that ref pointed at `n` moves ago.
/// Returns `None` if `spec` isn't a reflog revision, so callers can fall through to ordinary
/// revision parsing.
// Used by `resolve_commit_from_source`.
pub fn resolve_reflog_revision(spec: &str) -> Result<Option<String>> {
    let Some((name, index)) = parse_reflog_spec(spec) else {
        return Ok(None);
    };

    let ref_name = resolve_reflog_ref_name(&name);
    let entries = read_reflog(&ref_name)?;
    let Some(entry) = entries.get(index) else {
        if entries.is_empty() {
            anyhow::bail!("fatal: log for '{}' is empty or missing (reflogs only start when a repository first runs a reflog-aware rgit)", name);
        }
        anyhow::bail!("fatal: log for '{}' only has {} entr{}", name, entries.len(), if entries.len() == 1 { "y" } else { "ies" });
    };
    if entry.new_hash == NULL_HASH {
        anyhow::bail!("fatal: '{}' no longer points at a commit in the reflog", spec);
    }
    Ok(Some(entry.new_hash.clone()))
}

/// Drops entries older than `cutoff` (a Unix timestamp) from a ref's reflog and returns how
/// many were removed. `None` keeps everything.
// Used by the `reflog` command (`expire`).
pub fn expire_reflog(ref_name: &str, cutoff: Option<u64>) -> Result<usize> {
    let Some(cutoff) = cutoff else {
        return Ok(0);
    };
    let entries = read_reflog(ref_name)?;
    let before = entries.len();
    let kept: Vec<ReflogEntry> = entries.into_iter().filter(|e| e.timestamp >= cutoff).collect();
    let removed = before - kept.len();
    if removed > 0 {
        write_reflog(ref_name, &kept)?;
    }
    Ok(removed)
}

/// Renders a reflog the way `reflog show` prints it, one line per entry:
/// `<short hash> <name>@{<n>}: <message>`, newest first, at most `limit` entries.
// Used by the `reflog` command (`show`).
pub fn format_reflog_output(display_name: &str, entries: &[ReflogEntry], limit: Option<usize>) -> String {
    let mut out = String::new();
    for (n, entry) in entries.iter().enumerate().take(limit.unwrap_or(usize::MAX)) {
        out.push_str(&format!(
            "{} {}@{{{}}}: {}\n",
            entry.new_hash.chars().take(7).collect::<String>(),
            display_name,
            n,
            entry.message
        ));
    }
    out
}