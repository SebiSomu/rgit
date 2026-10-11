use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use crate::helpers::checkout::{sync_working_tree, update_index_from_tree};
use crate::helpers::fetch::read_remote_head;
use crate::helpers::github::{build_github_client, get_github_default_branch, parse_github_remote, resolve_github_token};
use crate::helpers::push::resolve_remote_repo_path;
use crate::helpers::resolve_tree_from_source;
use crate::objects::{CloneSource, GitHubTarget};

/// Classifies the repository argument of `clone` as a local repository or a GitHub
/// repository. A path that exists on disk is always treated as local (even if it contains
/// `github.com/`, e.g. `~/src/github.com/owner/repo`) and is made absolute here, so that it
/// stays valid after `clone` changes into the new directory.
// Used by `clone`.
pub fn classify_clone_source(spec: &str) -> Result<CloneSource> {
    let spec = spec.trim();
    if spec.is_empty() {
        anyhow::bail!("fatal: you must specify a repository to clone");
    }

    let as_path = Path::new(spec);
    if as_path.exists() {
        let absolute = std::path::absolute(as_path)
            .with_context(|| format!("Cannot resolve path '{}'", spec))?;
        let git_dir = resolve_remote_repo_path(&absolute.to_string_lossy())?;
        return Ok(CloneSource::Local { path: absolute, git_dir });
    }

    if let Some((owner, repo)) = parse_github_remote(spec) {
        return Ok(CloneSource::GitHub { owner, repo });
    }

    anyhow::bail!("fatal: repository '{}' does not exist", spec)
}

/// Looks up the branch the source repository's HEAD points to: the `HEAD` file of a local
/// repository, or the `default_branch` the GitHub API reports. Returns `None` when a local
/// source has a detached HEAD. Run before anything is created, so a missing GitHub token or
/// an unreachable repository fails the clone before it leaves any files behind.
// Used by `clone`.
pub fn remote_default_branch(source: &CloneSource, remote_name: &str) -> Result<Option<String>> {
    match source {
        CloneSource::Local { git_dir, .. } => read_remote_head(git_dir),
        CloneSource::GitHub { owner, repo } => {
            let token = resolve_github_token(remote_name)?;
            let client = build_github_client(&token)?;
            let target = GitHubTarget { owner: owner.clone(), repo: repo.clone(), token };
            Ok(Some(get_github_default_branch(&client, &target)?))
        }
    }
}

/// Decides which remote branch to check out after the fetch: the one requested with `-b`
/// (an error if the remote doesn't have it), else the remote's default branch, else `main`,
/// else `master`, else the alphabetically first. `None` means the remote has no branches.
// Used by `clone`.
pub fn choose_checkout_branch(requested: Option<&str>, remote_default: Option<&str>, available: &[String], remote_name: &str) -> Result<Option<String>> {
    if let Some(wanted) = requested {
        if available.iter().any(|b| b == wanted) {
            return Ok(Some(wanted.to_string()));
        }
        anyhow::bail!("fatal: Remote branch {} not found in upstream {}", wanted, remote_name);
    }

    if available.is_empty() {
        return Ok(None);
    }

    if let Some(default) = remote_default {
        if available.iter().any(|b| b == default) {
            return Ok(Some(default.to_string()));
        }
    }

    for candidate in ["main", "master"] {
        if available.iter().any(|b| b == candidate) {
            return Ok(Some(candidate.to_string()));
        }
    }

    Ok(available.iter().min().cloned())
}

/// Makes sure the clone destination can be used: creates it if missing, accepts an existing
/// empty directory, and refuses anything else. Returns whether the directory was created by
/// this call, so a failed clone knows whether to delete it or only empty it.
// Used by `clone`.
pub fn prepare_clone_directory(dir: &Path, display_name: &str) -> Result<bool> {
    if dir.exists() {
        if !dir.is_dir() {
            anyhow::bail!("fatal: destination path '{}' already exists and is not a directory.", display_name);
        }
        let has_entries = fs::read_dir(dir)
            .with_context(|| format!("Cannot read directory '{}'", display_name))?
            .next()
            .is_some();
        if has_entries {
            anyhow::bail!("fatal: destination path '{}' already exists and is not an empty directory.", display_name);
        }
        return Ok(false);
    }

    fs::create_dir_all(dir).with_context(|| format!("Failed to create directory '{}'", display_name))?;
    Ok(true)
}

/// Undoes a failed clone: removes the destination if `clone` created it, otherwise empties
/// the pre-existing directory it was given. Errors are ignored, since this runs while
/// another error is already being reported. The current directory must not be inside `dir`.
// Used by `clone`.
pub fn cleanup_failed_clone(dir: &Path, created: bool) {
    if created {
        let _ = fs::remove_dir_all(dir);
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// Creates the bare skeleton of a repository in the current directory (object store, refs,
/// and a HEAD pointing at `main`), exactly what `init` creates plus `refs/remotes`, but
/// without `init`'s "Initialized empty repository" message, which would be misleading in the
/// middle of a clone.
// Used by `clone`.
pub fn create_repository_skeleton() -> Result<()> {
    fs::create_dir_all(".git/objects").context("Failed to create .git/objects")?;
    fs::create_dir_all(".git/refs/heads").context("Failed to create .git/refs/heads")?;
    fs::create_dir_all(".git/refs/remotes").context("Failed to create .git/refs/remotes")?;
    fs::write(".git/HEAD", "ref: refs/heads/main\n").context("Failed to write .git/HEAD")?;
    Ok(())
}

/// Writes the files and the index for `commit_hash` into the current directory, which must
/// be a freshly created repository with an empty working tree and an empty index.
// Used by `clone`.
pub fn checkout_commit_into_empty_worktree(commit_hash: &str) -> Result<()> {
    let target_tree = resolve_tree_from_source(commit_hash)?;
    let empty_head_tree = BTreeMap::new();
    let empty_index: BTreeMap<String, [u8; 20]> = BTreeMap::new();

    sync_working_tree(&target_tree, &empty_head_tree, &empty_index)?;
    update_index_from_tree(&target_tree, &empty_head_tree)?;
    Ok(())
}