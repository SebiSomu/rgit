use anyhow::Result;
use std::fs;
use std::path::Path;

use crate::helpers::commit::{commit_subject_line, short_hash};
use crate::helpers::worktree::{
    create_worktree_admin_dir, delete_worktree_admin_dir, find_worktree, format_worktree_list,
    format_worktree_porcelain, list_worktrees, locked_worktree_message, move_worktree,
    populate_worktree, prune_worktrees, resolve_commit_ish, unique_worktree_id,
    worktree_has_local_changes, worktree_using_branch, worktrees_admin_root,
};
use crate::objects::{BranchCreation, WorktreeAction};
use crate::{refs, repo};

/// `rgit worktree add [-b|-B <new-branch>] [-d] [-f] [--lock [--reason <r>]] [-q] <path> [<commit-ish>]`
/// `rgit worktree list [--porcelain|-v]`
/// `rgit worktree remove [-f [-f]] <worktree>`
/// `rgit worktree move [-f [-f]] <worktree> <new-path>`
/// `rgit worktree lock [--reason <r>] <worktree>` / `rgit worktree unlock <worktree>`
/// `rgit worktree prune [-n] [-v]`
///
/// Manages extra working trees attached to the same repository, using git's on-disk layout so
/// the two tools can share them: `<path>/.git` is a file naming `.git/worktrees/<id>/`, which
/// holds that worktree's HEAD, index and in-progress state, while objects, refs and config stay
/// shared. A branch can be checked out in only one worktree at a time.
///
/// - `add` creates `<path>`, and checks out `<commit-ish>` in it: a local branch name checks
///   that branch out, anything else gives a detached HEAD. With no `<commit-ish>` it uses (or
///   creates from HEAD) a branch named after the last component of `<path>`.
/// - `list` shows each worktree's path, HEAD commit and branch, plus `locked` / `prunable`.
/// - `remove` deletes a clean linked worktree (`-f` for one with local changes, `-f -f` for a
///   locked one).
/// - `move` relocates a linked worktree; into an existing directory it keeps its name.
/// - `lock` / `unlock` protect a worktree (e.g. on removable media) from prune, move and remove.
/// - `prune` drops the records of worktrees whose directory was deleted by hand.
pub fn worktree(action: WorktreeAction) -> Result<()> {
    if !repo::exists() {
        anyhow::bail!("fatal: not a git repository (or any of the parent directories): .git");
    }

    match action {
        WorktreeAction::Add { path, commit_ish, new_branch, force_new_branch, detach, force, lock, reason, quiet } => {
            let lock_reason = lock.then(|| reason.unwrap_or_default());
            worktree_add(&path, commit_ish, new_branch, force_new_branch, detach, force, lock_reason, quiet)?;
        }

        WorktreeAction::List { porcelain, verbose } => {
            let worktrees = list_worktrees()?;
            if porcelain {
                print!("{}", format_worktree_porcelain(&worktrees));
            } else {
                print!("{}", format_worktree_list(&worktrees, verbose));
            }
        }

        WorktreeAction::Remove { worktree, force } => {
            let wt = find_worktree(&list_worktrees()?, &worktree)?;
            if wt.is_main() {
                anyhow::bail!("fatal: '{}' is a main working tree", worktree);
            }
            if wt.is_current {
                anyhow::bail!("fatal: cannot remove the current working tree");
            }
            if let Some(reason) = &wt.locked {
                if force < 2 {
                    anyhow::bail!("{}\nuse 'remove -f -f' to override or unlock first", locked_worktree_message("remove", reason));
                }
            }
            if wt.path.exists() {
                if force == 0 && worktree_has_local_changes(&wt.path)? {
                    anyhow::bail!("fatal: '{}' contains modified or untracked files, use --force to delete it", worktree);
                }
                fs::remove_dir_all(&wt.path)
                    .map_err(|e| anyhow::anyhow!("fatal: failed to delete '{}': {}", wt.display_path(), e))?;
            }
            delete_worktree_admin_dir(&wt.admin_dir)?;
        }

        WorktreeAction::Move { worktree, new_path, force } => {
            let wt = find_worktree(&list_worktrees()?, &worktree)?;
            if wt.is_main() {
                anyhow::bail!("fatal: '{}' is a main working tree", worktree);
            }
            if wt.is_current {
                anyhow::bail!("fatal: cannot move the current working tree");
            }
            if let Some(reason) = &wt.locked {
                if force < 2 {
                    anyhow::bail!("{}\nuse 'move -f -f' to override or unlock first", locked_worktree_message("move", reason));
                }
            }
            if !wt.path.exists() {
                anyhow::bail!("fatal: validation failed, cannot move working tree: '{}' does not exist", wt.display_path());
            }

            let mut destination = repo::normalize(&new_path);
            if destination.is_dir() {
                if let Some(name) = wt.path.file_name() {
                    destination = destination.join(name);
                }
            }
            if destination.exists() {
                anyhow::bail!("fatal: '{}' already exists", repo::to_git_path_string(&destination));
            }
            move_worktree(&wt, &destination)?;
        }

        WorktreeAction::Lock { worktree, reason } => {
            let wt = find_worktree(&list_worktrees()?, &worktree)?;
            if wt.is_main() {
                anyhow::bail!("fatal: The main working tree cannot be locked or unlocked");
            }
            match wt.locked.as_deref() {
                Some("") => anyhow::bail!("fatal: '{}' is already locked", worktree),
                Some(old) => anyhow::bail!("fatal: '{}' is already locked, reason: {}", worktree, old),
                None => fs::write(wt.admin_dir.join("locked"), reason.unwrap_or_default())?,
            }
        }

        WorktreeAction::Unlock { worktree } => {
            let wt = find_worktree(&list_worktrees()?, &worktree)?;
            if wt.is_main() {
                anyhow::bail!("fatal: The main working tree cannot be locked or unlocked");
            }
            if wt.locked.is_none() {
                anyhow::bail!("fatal: '{}' is not locked", worktree);
            }
            fs::remove_file(wt.admin_dir.join("locked"))?;
        }

        WorktreeAction::Prune { dry_run, verbose } => {
            for (id, reason) in prune_worktrees(dry_run)? {
                if dry_run || verbose {
                    println!("Removing worktrees/{}: {}", id, reason);
                }
            }
        }
    }

    Ok(())
}

/// `worktree add`: works out which branch or commit to check out, creates the directory and its
/// administrative files, creates or resets the branch, then checks the commit out. Anything
/// created is rolled back if a later step fails.
#[allow(clippy::too_many_arguments)]
fn worktree_add(
    path: &Path,
    commit_ish: Option<String>,
    new_branch: Option<String>,
    force_new_branch: Option<String>,
    detach: bool,
    force: bool,
    lock_reason: Option<String>,
    quiet: bool,
) -> Result<()> {
    let target = repo::normalize(path);
    let display = path.display().to_string();

    // A path that is still registered although its directory is gone.
    if let Some(stale) = list_worktrees()?.into_iter().find(|w| !w.is_main() && w.path == target) {
        if !target.exists() {
            if stale.locked.is_some() || !force {
                anyhow::bail!(
                    "fatal: '{}' is a missing but {}registered worktree;\nuse 'add -f' to override, or 'prune' or 'remove' to clear",
                    display,
                    if stale.locked.is_some() { "locked " } else { "already " }
                );
            }
            delete_worktree_admin_dir(&stale.admin_dir)?;
        }
    }
    let target_existed = target.exists();
    if target_existed && fs::read_dir(&target).map(|mut d| d.next().is_some()).unwrap_or(true) {
        anyhow::bail!("fatal: '{}' already exists", display);
    }

    let head_commit = || -> Result<String> {
        refs::resolve_head_commit()?.ok_or_else(|| anyhow::anyhow!("fatal: invalid reference: HEAD (no commits yet)"))
    };

    // (branch to check out, commit, how the branch comes to be)
    let (branch, commit, creation) = if let Some(name) = new_branch.or(force_new_branch.clone()) {
        let start = match &commit_ish {
            Some(c) => resolve_commit_ish(c)?,
            None => head_commit()?,
        };
        let existing = refs::read_ref(&format!("refs/heads/{}", name))?;
        let creation = match existing {
            Some(_) if force_new_branch.is_none() => anyhow::bail!("fatal: a branch named '{}' already exists", name),
            Some(old) => BranchCreation::Reset(old),
            None => BranchCreation::New,
        };
        (Some(name), start, creation)
    } else if detach {
        let commit = match &commit_ish {
            Some(c) => resolve_commit_ish(c)?,
            None => head_commit()?,
        };
        (None, commit, BranchCreation::Existing)
    } else if let Some(c) = &commit_ish {
        match refs::read_ref(&format!("refs/heads/{}", c))? {
            Some(tip) => (Some(c.clone()), tip, BranchCreation::Existing),
            None => (None, resolve_commit_ish(c)?, BranchCreation::Existing),
        }
    } else {
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or_else(|| anyhow::anyhow!("fatal: cannot derive a branch name from '{}'", display))?;
        match refs::read_ref(&format!("refs/heads/{}", name))? {
            Some(tip) => (Some(name), tip, BranchCreation::Existing),
            None => (Some(name), head_commit()?, BranchCreation::New),
        }
    };

    if let Some(name) = &branch {
        if !refs::is_valid_branch_name(name) {
            anyhow::bail!("fatal: '{}' is not a valid branch name", name);
        }
        let branch_ref = format!("refs/heads/{}", name);
        if let Some(user) = worktree_using_branch(&branch_ref, true)? {
            match creation {
                BranchCreation::Reset(_) => anyhow::bail!(
                    "fatal: cannot force update the branch '{}' used by worktree at '{}'",
                    name,
                    user.display_path()
                ),
                BranchCreation::Existing if !force => anyhow::bail!(
                    "fatal: '{}' is already used by worktree at '{}'",
                    name,
                    user.display_path()
                ),
                _ => {}
            }
        }
    }

    if !quiet {
        match (&branch, &creation) {
            (Some(name), BranchCreation::New) => println!("Preparing worktree (new branch '{}')", name),
            (Some(name), BranchCreation::Reset(old)) => {
                println!("Preparing worktree (resetting branch '{}'; was at {})", name, short_hash(old))
            }
            (Some(name), BranchCreation::Existing) => println!("Preparing worktree (checking out '{}')", name),
            (None, _) => println!("Preparing worktree (detached HEAD {})", short_hash(&commit)),
        }
    }

    fs::create_dir_all(&target).map_err(|e| anyhow::anyhow!("fatal: could not create directory of '{}': {}", display, e))?;
    let head_content = match &branch {
        Some(name) => format!("ref: refs/heads/{}", name),
        None => commit.clone(),
    };
    let id = unique_worktree_id(&target);

    let result = (|| -> Result<()> {
        create_worktree_admin_dir(&target, &id, &head_content, lock_reason.as_deref())?;
        if let Some(name) = &branch {
            match creation {
                BranchCreation::New => refs::create_branch(name, &commit)?,
                BranchCreation::Reset(_) => refs::write_ref(&format!("refs/heads/{}", name), &commit)?,
                BranchCreation::Existing => {}
            }
        }
        populate_worktree(&target, &commit)
    })();

    if let Err(err) = result {
        let _ = delete_worktree_admin_dir(&worktrees_admin_root().join(&id));
        let _ = fs::remove_dir_all(&target);
        if target_existed {
            let _ = fs::create_dir_all(&target);
        }
        if let (Some(name), BranchCreation::New) = (&branch, &creation) {
            let _ = refs::delete_branch(name);
        }
        return Err(err);
    }

    if !quiet {
        println!("HEAD is now at {} {}", short_hash(&commit), commit_subject_line(&commit).unwrap_or_default());
    }
    Ok(())
}
