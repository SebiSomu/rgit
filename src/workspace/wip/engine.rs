//! The WIP stack of `rgit wip`: one hidden ref per branch, `refs/rgit/wip/<branch>`, pointing at
//! the newest save. Every save is a stash-shaped commit whose parents are the commit it was made
//! on, its index commit, its untracked-files commit and (fourth) the save below it, so the whole
//! stack stays reachable from the one ref, for rgit's gc and for real git alike.

use anyhow::{Result, bail};

use crate::core::object_store::{read_object, write_object};
use crate::core::refs;
use crate::workspace::wip::types::WipEntry;

/// Where the WIP stacks live.
pub const WIP_PREFIX: &str = "refs/rgit/wip/";

/// The ref holding `branch`'s WIP stack.
// Used by every `rgit wip` action.
pub fn wip_ref(branch: &str) -> String {
    format!("{}{}", WIP_PREFIX, branch)
}

/// The branch HEAD is on; a detached HEAD has no WIP stack.
// Used by `rgit wip` (save, `--restore`, `--drop`, `--list`).
pub fn current_branch() -> Result<Option<String>> {
    Ok(match refs::resolve_head()? {
        refs::HeadState::Branch(b) => Some(b),
        refs::HeadState::Detached(_) => None,
    })
}

/// Like `current_branch`, but fails on a detached HEAD with a hint.
// Used by `rgit wip` (save, `--restore`, `--drop`).
pub fn require_branch() -> Result<String> {
    match current_branch()? {
        Some(b) => Ok(b),
        None => crate::ui::die_fatal("rgit wip saves per branch, and HEAD is detached\nhint: switch to a branch first, or use `rgit stash`"),
    }
}

/// `branch`'s WIP saves, newest first (`wip@{0}` == `[0]`).
// Used by every `rgit wip` action.
pub fn read_stack(branch: &str) -> Result<Vec<WipEntry>> {
    let mut stack = Vec::new();
    let mut next = refs::read_ref(&wip_ref(branch))?;
    while let Some(hash) = next {
        let entry = parse_entry(&hash)?;
        next = entry.below.clone();
        stack.push(entry);
    }
    Ok(stack)
}

/// Reads one WIP save commit.
// Used by `read_stack`.
fn parse_entry(hash: &str) -> Result<WipEntry> {
    let (kind, content) = read_object(hash)?;
    let text = String::from_utf8_lossy(&content).to_string();
    let parents = crate::core::commit::commit_parents(&text);
    if kind != "commit" || parents.len() < 3 {
        bail!("fatal: {} is not a WIP save (corrupted WIP stack?)", &hash[..hash.len().min(7)]);
    }
    let (header, body) = text.split_once("\n\n").unwrap_or((text.as_str(), ""));
    let time = header
        .lines()
        .find_map(|l| l.strip_prefix("committer "))
        .and_then(|l| l.rsplit(' ').nth(1))
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);
    Ok(WipEntry {
        commit: hash.to_string(),
        base: parents[0].clone(),
        index: parents[1].clone(),
        untracked: parents[2].clone(),
        below: parents.get(3).cloned(),
        message: body.lines().next().unwrap_or("").to_string(),
        time,
    })
}

/// Points `branch`'s WIP ref at `top`, or deletes it when the stack is now empty.
// Used by `rgit wip` (save) and `remove_entry`.
pub fn set_top(branch: &str, top: Option<&str>) -> Result<()> {
    let name = wip_ref(branch);
    match top {
        Some(hash) => refs::write_ref(&name, hash),
        None => {
            let path = crate::core::repo::path(format!(".git/{}", name));
            if path.exists() {
                std::fs::remove_file(&path)?;
                refs::prune_empty_ref_dirs(&path);
            }
            refs::remove_packed_ref(&name)?;
            Ok(())
        }
    }
}

/// Takes save `n` out of `branch`'s stack and returns it. The saves above it are rewritten so
/// they link to the one below it (same trees, messages and dates; only their fourth parent
/// changes).
// Used by `rgit wip --restore` and `--drop`.
pub fn remove_entry(branch: &str, n: usize) -> Result<WipEntry> {
    let stack = read_stack(branch)?;
    let removed = stack[n].clone();
    let mut top = removed.below.clone();
    for entry in stack[..n].iter().rev() {
        top = Some(relink(entry, top.as_deref())?);
    }
    set_top(branch, top.as_deref())?;
    Ok(removed)
}

/// Rewrites a WIP save commit with `below` as the save under it.
// Used by `remove_entry`.
fn relink(entry: &WipEntry, below: Option<&str>) -> Result<String> {
    let (_, content) = read_object(&entry.commit)?;
    let text = String::from_utf8_lossy(&content).to_string();
    let (header, body) = text.split_once("\n\n").unwrap_or((text.as_str(), ""));
    let mut out = String::new();
    for line in header.lines().filter(|l| !l.starts_with("parent ")) {
        out.push_str(line);
        out.push('\n');
        if line.starts_with("tree ") {
            for parent in [Some(entry.base.as_str()), Some(entry.index.as_str()), Some(entry.untracked.as_str()), below].into_iter().flatten() {
                out.push_str(&format!("parent {}\n", parent));
            }
        }
    }
    out.push('\n');
    out.push_str(body);
    write_object("commit", out.as_bytes())
}

/// Every branch that has WIP saves, sorted.
// Used by `rgit wip --list --all` and the hints when the current branch has none.
pub fn branches_with_saves() -> Vec<String> {
    refs::list_all_refs().into_keys().filter_map(|name| name.strip_prefix(WIP_PREFIX).map(str::to_string)).collect()
}

/// Parses `wip@{<n>}`, a bare `<n>`, or nothing (`wip@{0}`).
// Used by `rgit wip --restore` and `--drop`.
pub fn parse_position(raw: Option<&str>) -> Result<usize> {
    let Some(raw) = raw else { return Ok(0) };
    let inner = raw.trim().strip_prefix("wip@{").and_then(|r| r.strip_suffix('}')).unwrap_or(raw.trim());
    inner.parse().map_err(|_| anyhow::anyhow!("fatal: '{}' is not a WIP save (use wip@{{<n>}} or <n>)", raw))
}
