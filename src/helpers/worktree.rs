use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::helpers::checkout::ensure_working_tree_clean;
use crate::helpers::clone::checkout_commit_into_empty_worktree;
use crate::helpers::ls_files::collect_untracked_paths;
use crate::helpers::commit::short_hash;
use crate::helpers::reflog::{record_ref_update, NULL_HASH};
use crate::helpers::rev_parse::{peel_to_type, resolve_rev};
use crate::index::read_index;
use crate::objects::WorktreeInfo;
use crate::repo;

/// The directory under the shared `.git` that holds one administrative directory per linked
/// worktree.
// Used by `list_worktrees`, `all_worktree_git_dirs`, `create_worktree_admin_dir` and `prune_worktrees`.
pub fn worktrees_admin_root() -> PathBuf {
    repo::normalize(&repo::common_dir()).join("worktrees")
}

/// Returns the top directory of the main worktree (the parent of the shared `.git`).
// Used by `list_worktrees`.
pub fn main_worktree_path() -> PathBuf {
    let common = repo::normalize(&repo::common_dir());
    common.parent().map(Path::to_path_buf).unwrap_or(common)
}

/// Returns the `.git`-like directory of every worktree, main first: the shared `.git`, then
/// each `.git/worktrees/<id>`. Whatever a worktree keeps privately (HEAD, index, merge or rebase
/// state) lives in one of these.
// Used by `gc` and `fsck` (roots from every worktree) and `rename_branch` (HEADs elsewhere).
pub fn all_worktree_git_dirs() -> Result<Vec<PathBuf>> {
    let mut dirs = vec![repo::normalize(&repo::common_dir())];
    let root = worktrees_admin_root();
    if root.is_dir() {
        let mut linked: Vec<PathBuf> = fs::read_dir(&root)
            .with_context(|| format!("Failed to read {}", root.display()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        linked.sort();
        dirs.extend(linked);
    }
    Ok(dirs)
}

/// Reads a worktree's HEAD file into `(commit, branch ref)`: an attached HEAD yields the ref
/// (`refs/heads/x`) and whatever commit it holds (`None` when unborn); a detached HEAD yields
/// just the commit.
// Used by `list_worktrees`.
fn read_worktree_head(admin_dir: &Path) -> (Option<String>, Option<String>) {
    let content = match fs::read_to_string(admin_dir.join("HEAD")) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    let content = content.trim();
    match content.strip_prefix("ref: ") {
        Some(ref_name) => {
            let commit = fs::read_to_string(repo::path(format!(".git/{}", ref_name)))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            (commit, Some(ref_name.to_string()))
        }
        None => (Some(content.to_string()).filter(|s| !s.is_empty()), None),
    }
}

/// Reads the `gitdir` file of a linked worktree's admin directory and returns the worktree's
/// top directory, or why the record is stale (git's wording for `prunable`).
// Used by `list_worktrees` and `prune_worktrees`.
fn linked_worktree_location(admin_dir: &Path) -> std::result::Result<PathBuf, (Option<PathBuf>, &'static str)> {
    let gitdir = match fs::read_to_string(admin_dir.join("gitdir")) {
        Ok(text) => text.trim().to_string(),
        Err(_) => return Err((None, "gitdir file does not exist")),
    };
    if gitdir.is_empty() {
        return Err((None, "invalid gitdir file"));
    }
    let dot_git = repo::normalize(Path::new(&gitdir));
    let top = dot_git.parent().map(Path::to_path_buf).unwrap_or_else(|| dot_git.clone());
    if !dot_git.exists() {
        return Err((Some(top), "gitdir file points to non-existent location"));
    }
    Ok(top)
}

/// Lists every worktree of the repository, the main one first and then the linked ones
/// sorted by id, with HEAD, lock and prunable state filled in.
// Used by the `worktree` command, `find_worktree` and `worktree_using_branch`.
pub fn list_worktrees() -> Result<Vec<WorktreeInfo>> {
    let cwd = std::env::current_dir().map(|d| repo::normalize(&d)).unwrap_or_default();
    let common = repo::normalize(&repo::common_dir());

    let main_path = main_worktree_path();
    let (head, branch) = read_worktree_head(&common);
    let mut out = vec![WorktreeInfo {
        is_current: !repo::is_linked_worktree() && cwd.starts_with(&main_path),
        path: main_path,
        admin_dir: common,
        id: None,
        head,
        branch,
        locked: None,
        prunable: None,
    }];

    for admin_dir in all_worktree_git_dirs()?.into_iter().skip(1) {
        let id = admin_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let (path, prunable) = match linked_worktree_location(&admin_dir) {
            Ok(top) => (top, None),
            Err((top, reason)) => (top.unwrap_or_else(|| admin_dir.clone()), Some(reason.to_string())),
        };
        let locked = fs::read_to_string(admin_dir.join("locked")).ok().map(|r| r.trim().to_string());
        let (head, branch) = read_worktree_head(&admin_dir);
        out.push(WorktreeInfo {
            is_current: prunable.is_none() && cwd.starts_with(&path),
            path,
            admin_dir,
            id: Some(id),
            head,
            branch,
            // A locked worktree is never prunable, even when its directory is missing.
            prunable: if locked.is_some() { None } else { prunable },
            locked,
        });
    }

    // A linked worktree nested inside the main one also "contains" the cwd; the deepest wins.
    if let Some(deepest) = out
        .iter()
        .enumerate()
        .filter(|(_, w)| w.is_current)
        .max_by_key(|(_, w)| w.path.components().count())
        .map(|(i, _)| i)
    {
        for (i, w) in out.iter_mut().enumerate() {
            w.is_current = i == deepest;
        }
    }

    Ok(out)
}

/// Finds the worktree a user named, either by its path (relative or absolute) or by trailing
/// path components that are unique among all worktrees (`feature` for `../wt/feature`).
// Used by `worktree remove`, `move`, `lock` and `unlock`.
pub fn find_worktree(worktrees: &[WorktreeInfo], spec: &str) -> Result<WorktreeInfo> {
    let wanted = repo::normalize(Path::new(spec));
    if let Some(found) = worktrees.iter().find(|w| w.path == wanted) {
        return Ok(found.clone());
    }

    let suffix: Vec<_> = Path::new(spec).components().collect();
    let matches: Vec<&WorktreeInfo> = worktrees
        .iter()
        .filter(|w| {
            let parts: Vec<_> = w.path.components().collect();
            !suffix.is_empty() && parts.len() >= suffix.len() && parts[parts.len() - suffix.len()..] == suffix[..]
        })
        .collect();
    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => anyhow::bail!("fatal: '{}' is not a working tree", spec),
        _ => anyhow::bail!("fatal: '{}' is ambiguous; use the full path", spec),
    }
}

/// Returns a worktree whose HEAD is attached to `branch_ref` (skipping the current one unless
/// `include_current`), since one branch must never be checked out in two worktrees at once.
// Used by `worktree add`, `switch`/`checkout` and `branch -d`/`-D`.
pub fn worktree_using_branch(branch_ref: &str, include_current: bool) -> Result<Option<WorktreeInfo>> {
    Ok(list_worktrees()?
        .into_iter()
        .find(|w| (include_current || !w.is_current) && w.branch.as_deref() == Some(branch_ref)))
}

/// Picks the id of a new worktree's admin directory: the target directory's name made safe,
/// with a number appended if another worktree already uses it (`feature`, `feature1`, ...).
// Used by `worktree add`.
pub fn unique_worktree_id(path: &Path) -> String {
    let base: String = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '-' })
        .collect();
    let base = base.trim_start_matches('.').to_string();
    let base = if base.is_empty() { "worktree".to_string() } else { base };

    let root = worktrees_admin_root();
    if !root.join(&base).exists() {
        return base;
    }
    (1..).map(|n| format!("{}{}", base, n)).find(|id| !root.join(id).exists()).unwrap()
}

/// Runs `f` with the current directory switched to `dir`, then switches back even if `f`
/// failed. Every repository path resolves from the current directory, so this is how an
/// operation is pointed at another worktree.
// Used by `populate_worktree` and `worktree_has_local_changes`.
pub fn with_current_dir<T>(dir: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let original = std::env::current_dir().context("Cannot determine current directory")?;
    std::env::set_current_dir(dir).with_context(|| format!("Failed to enter {}", dir.display()))?;
    let result = f();
    std::env::set_current_dir(&original)
        .with_context(|| format!("Failed to return to {}", original.display()))?;
    result
}

/// Registers a new linked worktree: creates `.git/worktrees/<id>/` with `HEAD`, `commondir`,
/// `gitdir` (and `locked` if asked), and writes the `gitdir:` pointer file at `<path>/.git`.
/// `path` must already exist and be absolute. Returns the admin directory.
// Used by `worktree add`.
pub fn create_worktree_admin_dir(path: &Path, id: &str, head_content: &str, lock_reason: Option<&str>) -> Result<PathBuf> {
    let admin_dir = worktrees_admin_root().join(id);
    fs::create_dir_all(&admin_dir).with_context(|| format!("Failed to create {}", admin_dir.display()))?;

    fs::write(admin_dir.join("HEAD"), format!("{}\n", head_content))?;
    fs::write(admin_dir.join("commondir"), "../..\n")?;
    fs::write(admin_dir.join("gitdir"), format!("{}\n", repo::to_git_path_string(&path.join(".git"))))?;
    if let Some(reason) = lock_reason {
        fs::write(admin_dir.join("locked"), reason)?;
    }
    fs::write(path.join(".git"), format!("gitdir: {}\n", repo::to_git_path_string(&admin_dir)))
        .with_context(|| format!("Failed to write {}", path.join(".git").display()))?;

    Ok(admin_dir)
}

/// Fills a freshly registered, empty worktree with the files and index of `commit` and
/// records the checkout in its HEAD reflog.
// Used by `worktree add`.
pub fn populate_worktree(path: &Path, commit: &str) -> Result<()> {
    with_current_dir(path, || {
        checkout_commit_into_empty_worktree(commit)?;
        record_ref_update("HEAD", None, Some(commit));
        Ok(())
    })
}

/// Says whether a worktree has anything `remove` would lose: staged changes, modified or
/// deleted tracked files, or untracked files that aren't ignored.
// Used by `worktree remove`.
pub fn worktree_has_local_changes(path: &Path) -> Result<bool> {
    with_current_dir(path, || {
        if ensure_working_tree_clean("remove").is_err() {
            return Ok(true);
        }
        let tracked: BTreeSet<String> = read_index()?.into_iter().map(|e| e.path).collect();
        Ok(collect_untracked_paths(&tracked, false)?.iter().any(|u| !u.ignored))
    })
}

/// Deletes a linked worktree's administrative directory, and the `worktrees/` folder too
/// once it is empty.
// Used by `worktree remove` and `prune_worktrees`.
pub fn delete_worktree_admin_dir(admin_dir: &Path) -> Result<()> {
    if admin_dir.exists() {
        fs::remove_dir_all(admin_dir).with_context(|| format!("Failed to remove {}", admin_dir.display()))?;
    }
    let root = worktrees_admin_root();
    if fs::read_dir(&root).map(|mut d| d.next().is_none()).unwrap_or(false) {
        let _ = fs::remove_dir(&root);
    }
    Ok(())
}

/// Finds (and unless `dry_run`, deletes) the records of linked worktrees whose directory is
/// gone, skipping locked ones. Returns `(id, reason)` for each stale record.
// Used by `worktree prune`.
pub fn prune_worktrees(dry_run: bool) -> Result<Vec<(String, String)>> {
    let mut pruned = Vec::new();
    for admin_dir in all_worktree_git_dirs()?.into_iter().skip(1) {
        if admin_dir.join("locked").exists() {
            continue;
        }
        if let Err((_, reason)) = linked_worktree_location(&admin_dir) {
            let id = admin_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if !dry_run {
                delete_worktree_admin_dir(&admin_dir)?;
            }
            pruned.push((id, reason.to_string()));
        }
    }
    Ok(pruned)
}

/// Moves a linked worktree's directory and points its `gitdir` record at the new location.
// Used by `worktree move`.
pub fn move_worktree(worktree: &WorktreeInfo, destination: &Path) -> Result<()> {
    fs::rename(&worktree.path, destination).with_context(|| {
        format!("fatal: failed to move '{}' to '{}'", worktree.display_path(), repo::to_git_path_string(destination))
    })?;
    fs::write(
        worktree.admin_dir.join("gitdir"),
        format!("{}\n", repo::to_git_path_string(&destination.join(".git"))),
    )
    .context("Failed to update the worktree's gitdir file")?;
    Ok(())
}

/// Resolves a `<commit-ish>` argument (branch, tag, `HEAD~2`, short hash...) to a commit.
// Used by `worktree add`.
pub fn resolve_commit_ish(spec: &str) -> Result<String> {
    resolve_rev(spec)
        .and_then(|hash| peel_to_type(hash, "commit"))
        .map_err(|_| anyhow::anyhow!("fatal: invalid reference: {}", spec))
}

/// The first line of the error for touching a locked worktree, with its reason if it has one.
// Used by `worktree remove` and `worktree move`.
pub fn locked_worktree_message(verb: &str, reason: &str) -> String {
    if reason.is_empty() {
        format!("fatal: cannot {} a locked working tree;", verb)
    } else {
        format!("fatal: cannot {} a locked working tree, lock reason: {}", verb, reason)
    }
}

/// `worktree list`: `<path> <short hash> [<branch>]` (or `(detached HEAD)`) with paths padded
/// to one column, then `locked` / `prunable`. With `-v` their reasons go on indented lines.
// Used by `worktree list`.
pub fn format_worktree_list(worktrees: &[WorktreeInfo], verbose: bool) -> String {
    let width = worktrees.iter().map(|w| w.display_path().chars().count()).max().unwrap_or(0);
    let mut out = String::new();

    for wt in worktrees {
        let hash = wt.head.as_deref().map(short_hash).unwrap_or("0000000");
        let label = match &wt.branch {
            Some(b) => format!("[{}]", b.strip_prefix("refs/heads/").unwrap_or(b)),
            None => "(detached HEAD)".to_string(),
        };
        // Same layout as git: the path column is one wider than the longest path.
        let mut line = format!("{:<width$} {} {}", wt.display_path(), hash, label, width = width + 1);
        let mut details = Vec::new();

        if let Some(reason) = &wt.locked {
            if verbose && !reason.is_empty() {
                details.push(format!("\tlocked: {}", reason));
            } else {
                line.push_str(" locked");
            }
        }
        if let Some(reason) = &wt.prunable {
            if verbose {
                details.push(format!("\tprunable: {}", reason));
            } else {
                line.push_str(" prunable");
            }
        }

        out.push_str(&line);
        out.push('\n');
        for detail in details {
            out.push_str(&detail);
            out.push('\n');
        }
    }
    out
}

/// `worktree list --porcelain`: one stanza per worktree (`worktree`, `HEAD`, `branch` or
/// `detached`, optional `locked` / `prunable`), each followed by a blank line.
// Used by `worktree list --porcelain`.
pub fn format_worktree_porcelain(worktrees: &[WorktreeInfo]) -> String {
    let mut out = String::new();
    for wt in worktrees {
        out.push_str(&format!("worktree {}\n", wt.display_path()));
        out.push_str(&format!("HEAD {}\n", wt.head.as_deref().unwrap_or(NULL_HASH)));
        match &wt.branch {
            Some(b) => out.push_str(&format!("branch {}\n", b)),
            None => out.push_str("detached\n"),
        }
        match wt.locked.as_deref() {
            Some("") => out.push_str("locked\n"),
            Some(reason) => out.push_str(&format!("locked {}\n", reason)),
            None => {}
        }
        if let Some(reason) = &wt.prunable {
            out.push_str(&format!("prunable {}\n", reason));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn info(path: &str, branch: Option<&str>, locked: Option<&str>, prunable: Option<&str>) -> WorktreeInfo {
        WorktreeInfo {
            path: PathBuf::from(path),
            admin_dir: PathBuf::from("/repo/.git"),
            id: if path == "/repo" { None } else { Some("x".to_string()) },
            head: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            branch: branch.map(str::to_string),
            locked: locked.map(str::to_string),
            prunable: prunable.map(str::to_string),
            is_current: false,
        }
    }

    #[test]
    fn list_pads_paths_and_flags_states() {
        let wts = vec![
            info("/repo", Some("refs/heads/main"), None, None),
            info("/wt/feature", None, Some(""), None),
            info("/wt/gone", Some("refs/heads/gone"), None, Some("gitdir file points to non-existent location")),
        ];
        assert_eq!(
            format_worktree_list(&wts, false),
            "/repo        0123456 [main]\n\
             /wt/feature  0123456 (detached HEAD) locked\n\
             /wt/gone     0123456 [gone] prunable\n"
        );
        assert_eq!(
            format_worktree_list(&wts, true),
            "/repo        0123456 [main]\n\
             /wt/feature  0123456 (detached HEAD) locked\n\
             /wt/gone     0123456 [gone]\n\
             \tprunable: gitdir file points to non-existent location\n"
        );
    }

    #[test]
    fn porcelain_has_one_stanza_per_worktree() {
        let wts = vec![
            info("/repo", Some("refs/heads/main"), None, None),
            info("/wt/feature", None, Some("usb disk"), None),
        ];
        assert_eq!(
            format_worktree_porcelain(&wts),
            "worktree /repo\nHEAD 0123456789abcdef0123456789abcdef01234567\nbranch refs/heads/main\n\n\
             worktree /wt/feature\nHEAD 0123456789abcdef0123456789abcdef01234567\ndetached\nlocked usb disk\n\n"
        );
    }
}
