//! The operation log behind `rgit undo` / `rgit redo` (ideas file C.6).
//!
//! Every state-changing command is recorded automatically: `begin_recording` captures the
//! repository state before the command runs, `finish_recording` captures it afterwards and, if
//! anything changed, appends one line to `.git/rgit/oplog`. Undo / redo then restore exactly the
//! parts an operation changed, after checking that nobody changed those parts again since.
//!
//! The saved data (index copies, file contents, the stash list, in-progress files) lives in a
//! private store, `.git/rgit/objects`, never in `.git/objects`: git and rgit's git-identical
//! commands (fsck, count-objects, gc) see exactly the objects real git would.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use sha1::{Digest, Sha1};

use crate::core::object_store::{hash_content, read_object};
use crate::core::{refs, repo};
use crate::workspace::undo::types::{Captured, Change, OpKind, Operation, RepoState, RestorePlan};

/// Where the operation log lives (per worktree, since HEAD and the index are).
pub const OPLOG_PATH: &str = ".git/rgit/oplog";

/// The operation log's private object store: zlib-compressed contents named by their git blob id.
const STORE_PATH: &str = ".git/rgit/objects";

/// How many operations the log keeps; older ones are dropped.
const OPLOG_LIMIT: usize = 1000;

/// Untracked files bigger than this are left out of working-tree snapshots.
const MAX_SNAPSHOT_FILE: u64 = 64 * 1024 * 1024;

/// Set in the environment while a command is being recorded, so rgit processes it starts
/// (hooks, `rebase -x`, `bisect run`) don't record a second, overlapping operation.
const NESTED_ENV: &str = "RGIT_OPLOG_ACTIVE";

/// Commands that never change what undo restores; they are not captured at all (speed).
const READ_ONLY: &[&str] = &[
    "log", "show", "status", "diff", "blame", "annotate", "grep", "shortlog", "ls-files", "ls-tree", "cat-file",
    "rev-parse", "rev-list", "show-ref", "for-each-ref", "describe", "merge-base", "cherry", "whatchanged",
    "count-objects", "fsck", "fsck-objects", "check-ignore", "check-ref-format", "patch-id", "mailsplit", "mailinfo",
    "interpret-trailers", "check-mailmap", "fmt-merge-msg", "request-pull", "show-branch", "last-modified", "repo",
    "range-diff", "stripspace", "column", "var", "version", "help", "name-rev", "diff-tree", "diff-index",
    "diff-files", "show-index", "verify-pack", "conflicts", "archive", "format-patch", "get-tar-commit-id",
    "unpack-file", "merge-tree", "hash-object", "write-tree", "config", "clone", "init", "init-db", "push", "fetch",
    "gc", "prune", "repack", "pack-objects", "prune-packed", "index-pack", "unpack-objects", "commit-graph",
    "multi-pack-index", "maintenance", "for-each-repo", "fast-export", "bundle", "undo", "redo", "oops", "whereami", "story", "hotspots", "doctor", "stats", "timeline", "compare", "secrets",
];

/// Commands that can rewrite files in the working tree: their operations carry a snapshot.
const TOUCHES_WORKTREE: &[&str] = &[
    "reset", "checkout", "switch", "restore", "merge", "pull", "rebase", "cherry-pick", "revert", "stash", "clean",
    "rm", "mv", "am", "apply", "banana-split", "read-tree", "checkout-index", "sparse-checkout", "bisect", "merge-file",
    "rerere", "submodule", "fast-import", "wip", "save", "fixup", "sync", "absorb", "move-commit", "ignore",
];

/// The command being recorded: its command line, working directory and the state before it.
struct Pending {
    command: String,
    cwd: PathBuf,
    with_worktree: bool,
    before: Captured,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);

/// The subcommand name and the command line as typed, without rgit's global flags.
// Used by `begin_recording`.
fn command_line() -> (String, String) {
    let args: Vec<String> = std::env::args().skip(1).skip_while(|a| a.starts_with('-')).collect();
    let name = args.first().cloned().unwrap_or_default();
    let line = args
        .iter()
        .map(|a| if a.is_empty() || a.contains(char::is_whitespace) { format!("{:?}", a) } else { a.clone() })
        .collect::<Vec<_>>()
        .join(" ");
    (name, line)
}

/// Captures the state before a state-changing command runs (with a working-tree snapshot when
/// the command can rewrite files), so `finish_recording` can log the operation afterwards.
/// Does nothing outside a repository, for read-only commands and inside a recorded command.
// Used by `cli::handle_commands`.
pub fn begin_recording() {
    let (name, line) = command_line();
    if name.is_empty() || READ_ONLY.contains(&name.as_str()) || !repo::exists() || std::env::var_os(NESTED_ENV).is_some() {
        return;
    }
    let with_worktree = TOUCHES_WORKTREE.contains(&name.as_str());
    let Ok(before) = capture_state(with_worktree) else { return };
    unsafe { std::env::set_var(NESTED_ENV, "1") };
    if let Ok(mut p) = PENDING.lock() {
        *p = Some(Pending { command: line, cwd: std::env::current_dir().unwrap_or_default(), with_worktree, before });
    }
}

/// Captures the state after the command and, if anything undo cares about changed, appends
/// the operation to the log. Safe to call more than once (only the first call records).
// Used by `main` (normal return) and `ui::exit` (early exits).
pub fn finish_recording() {
    let Some(p) = PENDING.lock().ok().and_then(|mut g| g.take()) else { return };
    let _ = std::env::set_current_dir(&p.cwd);
    if !repo::exists() {
        return;
    }
    let Ok(after) = capture_state(p.with_worktree) else { return };
    if same_state(&p.before.state, &after.state) {
        return;
    }
    // The index copies are only saved when this operation changed the index.
    if p.before.state.index_key != after.state.index_key {
        for bytes in [&p.before.index_bytes, &after.index_bytes].into_iter().flatten() {
            if store_write(bytes).is_err() {
                return;
            }
        }
    }
    let _ = append_operation(OpKind::Command, &p.command, p.before.state, after.state);
}

/// Whether two states are the same for undo's purposes (the index is compared by content).
// Used by `finish_recording`.
fn same_state(a: &RepoState, b: &RepoState) -> bool {
    a.head == b.head && a.refs == b.refs && a.index_key == b.index_key && a.stash == b.stash && a.state_files == b.state_files && a.worktree == b.worktree
}

/// Reads HEAD, the local refs, the index, the stash list, the in-progress operation files and
/// (with `with_worktree`) a working-tree snapshot. The stash list, state files and changed file
/// contents are saved to the private store right away; saving the index is left to the caller.
// Used by `begin_recording`, `finish_recording` and the `undo` / `redo` commands.
pub fn capture_state(with_worktree: bool) -> Result<Captured> {
    let head = fs::read_to_string(repo::path(".git/HEAD")).context("failed to read .git/HEAD")?.trim().to_string();
    let index_bytes = fs::read(repo::path(".git/index")).ok();
    let (index, index_key) = match &index_bytes {
        Some(bytes) => (Some(hex::encode(hash_content("blob", bytes))), Some(index_fingerprint()?)),
        None => (None, None),
    };
    let stash = match fs::read(repo::path(".git/STASH_LIST")) {
        Ok(bytes) if !bytes.is_empty() => Some(store_write(&bytes)?),
        _ => None,
    };
    let state_files = state_files_snapshot()?;
    let worktree = if with_worktree { Some(store_manifest(&worktree_snapshot(true)?)?) } else { None };
    Ok(Captured {
        state: RepoState { head, refs: local_refs(), index, index_key, stash, state_files, worktree },
        index_bytes,
    })
}

/// Every direct ref (loose and packed, loose winning) except remote-tracking refs: those
/// mirror the remote, and undoing a local command must not move them.
// Used by `capture_state`.
fn local_refs() -> BTreeMap<String, String> {
    let mut all = BTreeMap::new();
    if let Ok(packed) = fs::read_to_string(repo::path(".git/packed-refs")) {
        for line in packed.lines() {
            if let Some((hash, name)) = line.split_once(' ')
                && hash.len() == 40
                && !line.starts_with('#')
                && !line.starts_with('^')
            {
                all.insert(name.to_string(), hash.to_string());
            }
        }
    }
    // Symbolic refs (refs/remotes/origin/HEAD) are collected separately and left out.
    let mut symbolic = Vec::new();
    refs::collect_loose_refs(&repo::path(".git/refs"), "refs", &mut all, &mut symbolic);
    all.retain(|name, _| !name.starts_with("refs/remotes/"));
    all
}

/// A fingerprint of the index's entries (stage, mode, object id, path), ignoring stat data.
// Used by `capture_state`.
fn index_fingerprint() -> Result<String> {
    let mut hasher = Sha1::new();
    for e in crate::core::index::read_index()? {
        hasher.update(format!("{} {:o} {} {}\n", e.stage, e.mode, hex::encode(e.hash), e.path).as_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Whether a top-level `.git` entry is in-progress operation state (MERGE_HEAD, ORIG_HEAD,
/// BISECT_*, RGIT_CONFLICTS, rebase-merge/, sequencer/, ...) that undo should put back.
// Used by `state_files_snapshot` and `restore_state_files`.
fn is_state_entry(name: &str, is_dir: bool) -> bool {
    if is_dir {
        return matches!(name, "sequencer" | "rebase-merge" | "rebase-apply");
    }
    let upper = !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_');
    upper && !matches!(name, "HEAD" | "FETCH_HEAD" | "STASH_LIST") && !name.ends_with("_EDITMSG")
}

/// The in-progress operation files of the current worktree's `.git`, relative path -> content.
// Used by `state_files_snapshot` and `restore_state_files`.
fn state_files() -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let name = format!("{}{}", prefix, entry.file_name().to_string_lossy());
            let path = entry.path();
            if path.is_dir() {
                walk(&path, &format!("{}/", name), out);
            } else if let Ok(bytes) = fs::read(&path) {
                out.insert(name, bytes);
            }
        }
    }
    let mut out = BTreeMap::new();
    let Ok(entries) = fs::read_dir(repo::git_dir()) else { return out };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if !is_state_entry(&name, path.is_dir()) {
            continue;
        }
        if path.is_dir() {
            walk(&path, &format!("{}/", name), &mut out);
        } else if let Ok(bytes) = fs::read(&path) {
            out.insert(name, bytes);
        }
    }
    out
}

/// Saves the in-progress operation files as a manifest (None when there are none).
// Used by `capture_state`.
fn state_files_snapshot() -> Result<Option<String>> {
    let files = state_files();
    if files.is_empty() {
        return Ok(None);
    }
    let mut map = BTreeMap::new();
    for (name, bytes) in files {
        let id = store_write(&bytes)?;
        map.insert(name, (hex_to_bytes(&id), 0o100644));
    }
    Ok(Some(store_manifest(&map)?))
}

/// The working tree as path -> (clean blob id, mode): tracked files (except submodules and
/// sparse-checkout gaps) and untracked, not-ignored files up to `MAX_SNAPSHOT_FILE`. Contents
/// are cleaned like `add` would (line endings), so restoring writes them back like a checkout.
/// With `write`, contents not already in the index are saved to the private store (an unchanged
/// tracked file is just its index blob, which `.git/objects` already holds).
// Used by `capture_state` and `plan_restore`.
fn worktree_snapshot(write: bool) -> Result<BTreeMap<String, ([u8; 20], u32)>> {
    let entries = crate::core::index::read_index().unwrap_or_default();
    let mut map = BTreeMap::new();
    let mut tracked = BTreeSet::new();
    let store = |content: Vec<u8>, known: Option<[u8; 20]>| -> Result<[u8; 20]> {
        let id = hash_content("blob", &content);
        if write && known != Some(id) {
            store_write(&content)?;
        }
        Ok(id)
    };
    for e in &entries {
        tracked.insert(e.path.clone());
        if e.mode == 0o160000 || map.contains_key(&e.path) {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&e.path) else { continue };
        if !meta.is_file() {
            continue;
        }
        let Ok(content) = crate::core::eol::read_clean(&e.path) else { continue };
        let known = (e.stage == 0).then_some(e.hash);
        let id = store(content, known)?;
        let mode = crate::workspace::index_plumbing::worktree_file_mode(&meta, Some(e.mode));
        map.insert(e.path.clone(), (id, mode));
    }
    for path in crate::workspace::state_files::collect_untracked_files(&tracked, false).unwrap_or_default() {
        let Ok(meta) = fs::symlink_metadata(&path) else { continue };
        if !meta.is_file() || meta.len() > MAX_SNAPSHOT_FILE {
            continue;
        }
        let Ok(content) = crate::core::eol::read_clean(&path) else { continue };
        let id = store(content, None)?;
        map.insert(path, (id, crate::workspace::index_plumbing::worktree_file_mode(&meta, None)));
    }
    Ok(map)
}

/// Saves `content` in the private store under its git blob id (unless already there).
// Used by `capture_state`, `finish_recording`, `worktree_snapshot`, `store_manifest` and `undo` / `redo`.
pub fn store_write(content: &[u8]) -> Result<String> {
    let id = hex::encode(hash_content("blob", content));
    let path = repo::path(format!("{}/{}/{}", STORE_PATH, &id[..2], &id[2..]));
    if !path.exists() {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(content)?;
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, encoder.finish()?)?;
        fs::rename(&tmp, &path).context("failed to save to the operation log's store")?;
    }
    Ok(id)
}

/// Loads saved content by blob id: from the private store, else from `.git/objects` (where the
/// index blobs of unchanged tracked files live).
// Used by `tree_map`, `stash_count`, `write_worktree_file`, `restore_blob_file`, `restore_state_files`.
fn store_read(id: &str) -> Result<Vec<u8>> {
    let path = repo::path(format!("{}/{}/{}", STORE_PATH, id.get(..2).unwrap_or(""), id.get(2..).unwrap_or("")));
    if let Ok(compressed) = fs::read(&path) {
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::ZlibDecoder::new(&compressed[..]), &mut out)?;
        return Ok(out);
    }
    let (_, content) = read_object(id).with_context(|| format!("the operation log's copy {} is missing", &id[..id.len().min(7)]))?;
    Ok(content)
}

/// Saves a path -> (id, mode) map as a manifest (one `<mode> <id> <path>` line per file).
// Used by `capture_state` and `state_files_snapshot`.
fn store_manifest(map: &BTreeMap<String, ([u8; 20], u32)>) -> Result<String> {
    let mut text = String::new();
    for (path, (id, mode)) in map {
        text.push_str(&format!("{:o} {} {}\n", mode, hex::encode(id), path));
    }
    store_write(text.as_bytes())
}

/// Removes saved contents no operation in the log refers to any more (after the log is trimmed).
// Used by `append_operation`.
fn prune_store(ops: &[Operation]) {
    let mut keep: BTreeSet<String> = BTreeSet::new();
    for op in ops {
        for s in [&op.before, &op.after] {
            keep.extend([&s.index, &s.stash].into_iter().flatten().cloned());
            for manifest in [&s.state_files, &s.worktree].into_iter().flatten() {
                keep.insert(manifest.clone());
                if let Ok(map) = tree_map(Some(manifest)) {
                    keep.extend(map.values().map(|(id, _)| hex::encode(id)));
                }
            }
        }
    }
    let Ok(dirs) = fs::read_dir(repo::path(STORE_PATH)) else { return };
    for dir in dirs.flatten() {
        let prefix = dir.file_name().to_string_lossy().to_string();
        let Ok(files) = fs::read_dir(dir.path()) else { continue };
        for file in files.flatten() {
            if !keep.contains(&format!("{}{}", prefix, file.file_name().to_string_lossy())) {
                let _ = fs::remove_file(file.path());
            }
        }
        let _ = fs::remove_dir(dir.path());
    }
}

fn hex_to_bytes(id: &str) -> [u8; 20] {
    let mut out = [0u8; 20];
    if let Ok(bytes) = hex::decode(id)
        && bytes.len() == 20
    {
        out.copy_from_slice(&bytes);
    }
    out
}

/// Reads a saved manifest back into path -> (id, mode) (empty for None).
// Used by `plan_restore`, `restore_state_files`, `prune_store` and `oplog_object_ids`.
fn tree_map(manifest: Option<&str>) -> Result<BTreeMap<String, ([u8; 20], u32)>> {
    let mut map = BTreeMap::new();
    let Some(id) = manifest else { return Ok(map) };
    let text = String::from_utf8(store_read(id)?).context("unreadable operation-log manifest")?;
    for line in text.lines() {
        let mut parts = line.splitn(3, ' ');
        if let (Some(mode), Some(hash), Some(path)) = (parts.next(), parts.next(), parts.next()) {
            map.insert(path.to_string(), (hex_to_bytes(hash), u32::from_str_radix(mode, 8).unwrap_or(0o100644)));
        }
    }
    Ok(map)
}

// ----------------------------------------------------------------------------------------
// The log
// ----------------------------------------------------------------------------------------

/// Reads every operation in the log, oldest first (an unreadable line is skipped).
// Used by the `undo` / `redo` commands, `append_operation` and `oplog_object_ids`.
pub fn read_oplog() -> Vec<Operation> {
    let Ok(text) = fs::read_to_string(repo::path(OPLOG_PATH)) else { return Vec::new() };
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Appends an operation (next id, current time) and trims the log to `OPLOG_LIMIT` entries.
// Used by `finish_recording` and the `undo` / `redo` commands.
pub fn append_operation(kind: OpKind, command: &str, before: RepoState, after: RepoState) -> Result<u64> {
    let mut ops = read_oplog();
    let id = ops.last().map(|o| o.id + 1).unwrap_or(1);
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let op = Operation { id, time, command: command.to_string(), kind, before, after };
    let path = repo::path(OPLOG_PATH);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // Trimmed in batches of 100 so the private store is only swept now and then.
    if ops.len() >= OPLOG_LIMIT + 100 {
        ops.push(op);
        let keep = &ops[ops.len() - OPLOG_LIMIT..];
        let mut text = String::new();
        for o in keep {
            text.push_str(&serde_json::to_string(o)?);
            text.push('\n');
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, &path)?;
        prune_store(keep);
    } else {
        let mut file = fs::OpenOptions::new().create(true).append(true).open(&path).context("failed to open the operation log")?;
        writeln!(file, "{}", serde_json::to_string(&op)?)?;
    }
    Ok(id)
}

/// Replays the log into two stacks of operation ids: `done` (what `undo` undoes next, last =
/// newest) and `undone` (what `redo` brings back next, last = most recently undone). A new
/// command clears the redo stack, as in an editor.
// Used by the `undo` / `redo` commands.
pub fn undo_stacks(ops: &[Operation]) -> (Vec<u64>, Vec<u64>) {
    let (mut done, mut undone) = (Vec::new(), Vec::new());
    for op in ops {
        match op.kind {
            OpKind::Command => {
                done.push(op.id);
                undone.clear();
            }
            OpKind::Undo { target } => {
                done.retain(|&id| id != target);
                undone.push(target);
            }
            OpKind::Redo { target } => {
                undone.retain(|&id| id != target);
                done.push(target);
            }
        }
    }
    (done, undone)
}

/// The `.git/objects` ids the log relies on (commits refs pointed at, index blobs of files in
/// working-tree snapshots), so `gc` keeps them like reflog entries. Ids that only live in the
/// private store are listed too; gc's lenient walk skips what it can't find.
// Used by `collect_lenient_gc_roots` (gc).
pub fn oplog_object_ids() -> Vec<String> {
    let mut out = BTreeSet::new();
    for op in read_oplog() {
        for s in [&op.before, &op.after] {
            out.extend(s.refs.values().cloned());
            if s.head.len() == 40 {
                out.insert(s.head.clone());
            }
            if let Ok(map) = tree_map(s.worktree.as_deref()) {
                out.extend(map.values().map(|(id, _)| hex::encode(id)));
            }
        }
    }
    out.into_iter().collect()
}

// ----------------------------------------------------------------------------------------
// Planning and applying a restore
// ----------------------------------------------------------------------------------------

/// Works out what must change to take the repository from `from` (the state an operation left)
/// to `to` (the state to go back to), touching only what differs between the two. Everything it
/// would touch must still look like `from` in `current`; whatever doesn't is a conflict.
// Used by the `undo` / `redo` commands.
pub fn plan_restore(from: &RepoState, to: &RepoState, current: &RepoState) -> Result<RestorePlan> {
    let mut plan = RestorePlan::default();

    if from.worktree.is_some() && to.worktree.is_some() && from.worktree != to.worktree {
        let (a, b) = (tree_map(from.worktree.as_deref())?, tree_map(to.worktree.as_deref())?);
        let now = worktree_snapshot(false)?;
        let (mut write, mut remove) = (Vec::new(), Vec::new());
        let paths: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
        for path in paths {
            let (was, target) = (a.get(path), b.get(path));
            if was.map(|e| e.0) == target.map(|e| e.0) && was.map(|e| e.1) == target.map(|e| e.1) {
                continue;
            }
            if now.get(path).map(|e| e.0) != was.map(|e| e.0) {
                plan.conflicts.push(match (was, now.get(path)) {
                    (Some(_), None) => format!("'{}' was deleted after the operation", path),
                    (None, Some(_)) => format!("'{}' was created after the operation", path),
                    _ => format!("'{}' was modified after the operation", path),
                });
            }
            match target {
                Some((id, mode)) => write.push((path.clone(), *id, *mode)),
                None => remove.push(path.clone()),
            }
        }
        if !write.is_empty() || !remove.is_empty() {
            plan.changes.push(Change::Files { write, remove });
        }
    }

    if from.index_key != to.index_key {
        if current.index_key != from.index_key {
            plan.conflicts.push("the index (staging area) changed after the operation".into());
        }
        plan.changes.push(Change::Index { to: to.index.clone() });
    }

    if from.state_files != to.state_files {
        if current.state_files != from.state_files {
            plan.conflicts.push("an operation in progress (merge, rebase, ...) started or changed after it".into());
        }
        plan.changes.push(Change::StateFiles {
            to: to.state_files.clone(),
            from_names: tree_map(from.state_files.as_deref())?.into_keys().collect(),
            to_names: tree_map(to.state_files.as_deref())?.into_keys().collect(),
        });
    }

    if from.stash != to.stash {
        if current.stash != from.stash {
            plan.conflicts.push("the stash list changed after the operation".into());
        }
        plan.changes.push(Change::Stash { to: to.stash.clone(), from_count: stash_count(&from.stash), to_count: stash_count(&to.stash) });
    }

    let names: BTreeSet<&String> = from.refs.keys().chain(to.refs.keys()).collect();
    for name in names {
        let (was, target) = (from.refs.get(name), to.refs.get(name));
        if was == target {
            continue;
        }
        let now = current.refs.get(name);
        if now != was {
            plan.conflicts.push(match now {
                Some(h) => format!("{} is now at {}, not {}", ref_label(name), &h[..7], was.map(|h| &h[..7]).unwrap_or("(absent)")),
                None => format!("{} was deleted after the operation", ref_label(name)),
            });
        }
        plan.changes.push(Change::Ref { name: name.clone(), from: was.cloned(), to: target.cloned() });
    }

    if from.head != to.head {
        if current.head != from.head {
            plan.conflicts.push(format!("HEAD is now {}, not {}", head_label(&current.head), head_label(&from.head)));
        }
        // HEAD would go back to a branch that was deleted since (and that this plan doesn't
        // recreate): that would leave HEAD on a branch with no commits.
        if let Some(target) = to.head.strip_prefix("ref: ")
            && to.refs.contains_key(target)
            && !current.refs.contains_key(target)
            && !plan.changes.iter().any(|c| matches!(c, Change::Ref { name, to: Some(_), .. } if name == target))
        {
            plan.conflicts.push(format!("{} was deleted after the operation", ref_label(target)));
        }
        plan.changes.push(Change::Head { from: from.head.clone(), to: to.head.clone() });
    }

    Ok(plan)
}

/// Number of entries in a stored stash list (0 for None).
// Used by `plan_restore`.
fn stash_count(blob: &Option<String>) -> usize {
    blob.as_deref().and_then(|b| store_read(b).ok()).map(|c| c.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count()).unwrap_or(0)
}

/// "branch 'main'", "tag 'v1'" or the full ref name, for messages.
// Used by `plan_restore` and `describe_change`.
pub fn ref_label(name: &str) -> String {
    if let Some(b) = name.strip_prefix("refs/heads/") {
        format!("branch '{}'", b)
    } else if let Some(t) = name.strip_prefix("refs/tags/") {
        format!("tag '{}'", t)
    } else {
        name.to_string()
    }
}

/// "branch 'main'" or "detached at 1a2b3c4", from the contents of `.git/HEAD`.
// Used by `plan_restore` and `describe_change`.
pub fn head_label(head: &str) -> String {
    match head.strip_prefix("ref: ") {
        Some(r) => ref_label(r),
        None => format!("detached at {}", &head[..head.len().min(7)]),
    }
}

/// Carries out a plan: working-tree files first, then the index, the in-progress files, the
/// stash list, refs and finally HEAD (so HEAD's reflog sees the restored branch).
// Used by the `undo` / `redo` commands.
pub fn apply_restore(plan: &RestorePlan) -> Result<()> {
    for change in &plan.changes {
        if let Change::Files { write, remove } = change {
            for path in remove {
                crate::workspace::state_files::remove_file_and_empty_parents(path);
            }
            for (path, id, mode) in write {
                write_worktree_file(path, id, *mode)?;
            }
        }
    }
    for change in &plan.changes {
        match change {
            Change::Index { to } => restore_blob_file(".git/index", to.as_deref())?,
            Change::Stash { to, .. } => restore_blob_file(".git/STASH_LIST", to.as_deref())?,
            Change::StateFiles { to, .. } => restore_state_files(to.as_deref())?,
            _ => {}
        }
    }
    for change in &plan.changes {
        if let Change::Ref { name, to, .. } = change {
            match to {
                Some(hash) => refs::write_ref(name, hash)?,
                None => delete_ref(name)?,
            }
        }
    }
    for change in &plan.changes {
        if let Change::Head { to, .. } = change {
            match to.strip_prefix("ref: refs/heads/") {
                Some(branch) => refs::set_head(branch)?,
                None if to.starts_with("ref: ") => fs::write(repo::path(".git/HEAD"), format!("{}\n", to))?,
                None => refs::set_head_detached(to)?,
            }
        }
    }
    crate::core::eol::reset_cache();
    Ok(())
}

/// Writes one working-tree file from its clean blob (line endings converted for checkout).
// Used by `apply_restore`.
fn write_worktree_file(path: &str, id: &[u8; 20], mode: u32) -> Result<()> {
    let content = store_read(&hex::encode(id)).with_context(|| format!("can't restore '{}'", path))?;
    if let Some(parent) = Path::new(path).parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    if fs::symlink_metadata(path).map(|m| m.is_dir()).unwrap_or(false) {
        anyhow::bail!("'{}' is a directory now; move it away and try again", path);
    }
    fs::write(path, crate::core::eol::smudge(path, &content)).with_context(|| format!("failed to write '{}'", path))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(if mode & 0o111 != 0 { 0o755 } else { 0o644 }))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

/// Puts a `.git` file back from a stored blob, or removes it for None.
// Used by `apply_restore` (index, stash list).
fn restore_blob_file(git_path: &str, blob: Option<&str>) -> Result<()> {
    let path = repo::path(git_path);
    match blob {
        Some(id) => {
            let content = store_read(id).with_context(|| format!("can't restore {}", git_path))?;
            fs::write(&path, content).with_context(|| format!("failed to write {}", git_path))?;
        }
        None => {
            if path.exists() {
                fs::remove_file(&path).with_context(|| format!("failed to remove {}", git_path))?;
            }
        }
    }
    Ok(())
}

/// Replaces the in-progress operation files of `.git` with the stored set (none for None).
// Used by `apply_restore`.
fn restore_state_files(tree: Option<&str>) -> Result<()> {
    let git_dir = repo::git_dir();
    for name in state_files().keys() {
        let _ = fs::remove_file(git_dir.join(name));
    }
    for dir in ["sequencer", "rebase-merge", "rebase-apply"] {
        let p = git_dir.join(dir);
        if p.is_dir() {
            let _ = fs::remove_dir_all(&p);
        }
    }
    for (name, (id, _)) in tree_map(tree)? {
        let content = store_read(&hex::encode(id))?;
        let path = git_dir.join(&name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)?;
    }
    Ok(())
}

/// Deletes a ref completely: loose file, packed entry, reflog and emptied directories.
// Used by `apply_restore`.
fn delete_ref(name: &str) -> Result<()> {
    let path = repo::path(format!(".git/{}", name));
    if path.is_file() {
        fs::remove_file(&path).with_context(|| format!("failed to delete {}", name))?;
    }
    refs::remove_packed_ref(name)?;
    crate::core::reflog::delete_reflog(name);
    refs::prune_empty_ref_dirs(&path);
    Ok(())
}
