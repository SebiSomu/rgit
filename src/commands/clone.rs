use anyhow::{Context, Result};
use std::path::Path;
use crate::helpers::clone::{
    checkout_commit_into_empty_worktree, choose_checkout_branch, classify_clone_source,
    cleanup_failed_clone, create_repository_skeleton, prepare_clone_directory,
    remote_default_branch,
};
use crate::helpers::config::{set_in, LOCAL_CONFIG_PATH};
use crate::helpers::push::set_upstream_branch;
use crate::objects::CloneSource;
use crate::refs;

/// `rgit clone [-b <branch>] [-o <name>] <repository> [<directory>]`
///
/// Creates `<directory>` (default: the repository's name), initialises a repository in it,
/// records the source as the remote (`origin`, or `-o <name>`), runs `fetch` for all of its
/// branches, then creates a local branch from the remote's default branch (or `-b`), checks
/// it out and sets it up to track its remote branch.
///
/// `<repository>` is a local path (a work tree or a bare repository) or a GitHub repository
/// (`https://github.com/owner/repo`, `git@github.com:owner/repo`, or `owner/repo`), which
/// needs a token exactly like `fetch` does. If anything fails after the directory has been
/// created, it is removed again.
pub fn clone(repository: String, directory: Option<String>, branch: Option<String>, origin: Option<String>) -> Result<()> {
    let remote_name = origin.unwrap_or_else(|| "origin".to_string());
    let source = classify_clone_source(&repository)?;
    let remote_default = remote_default_branch(&source, &remote_name)?;
    let dir_name = directory.unwrap_or_else(|| source.default_dir_name());
    let target_dir = std::path::absolute(&dir_name)
        .with_context(|| format!("Cannot resolve destination path '{}'", dir_name))?;
    let original_dir = std::env::current_dir().context("Cannot determine the current directory")?;

    let created = prepare_clone_directory(&target_dir, &dir_name)?;
    println!("Cloning into '{}'...", dir_name);

    let outcome = clone_into(&target_dir, &source, &remote_name, branch.as_deref(), remote_default.as_deref());
    let _ = std::env::set_current_dir(&original_dir);
    if let Err(err) = outcome {
        cleanup_failed_clone(&target_dir, created);
        return Err(err);
    }

    Ok(())
}

/// Does the work of a clone from inside the new directory, which every later step (config,
/// refs, objects, index) relies on being the current directory.
fn clone_into(target_dir: &Path, source: &CloneSource, remote_name: &str, requested: Option<&str>, remote_default: Option<&str>) -> Result<()> {
    std::env::set_current_dir(target_dir)
        .with_context(|| format!("Cannot enter '{}'", target_dir.display()))?;

    create_repository_skeleton()?;
    set_in(&crate::repo::path(LOCAL_CONFIG_PATH), &format!("remote.{}.url", remote_name), &source.url())?;

    crate::commands::fetch(Some(remote_name.to_string()), None)?;

    let available = refs::list_remote_tracking_branches(remote_name)?;
    match choose_checkout_branch(requested, remote_default, &available, remote_name)? {
        Some(name) => {
            let tracking_ref = format!("refs/remotes/{}/{}", remote_name, name);
            let commit_hash = refs::read_ref(&tracking_ref)?
                .ok_or_else(|| anyhow::anyhow!("fatal: remote branch '{}' was not fetched", name))?;

            refs::create_branch(&name, &commit_hash)?;
            refs::set_head(&name)?;
            checkout_commit_into_empty_worktree(&commit_hash)?;
            set_upstream_branch(&name, remote_name, &name)?;
        }
        None => {
            eprintln!("warning: You appear to have cloned an empty repository.");
            if let Some(default) = remote_default {
                if refs::is_valid_branch_name(default) {
                    refs::set_head(default)?;
                    set_upstream_branch(default, remote_name, default)?;
                }
            }
        }
    }

    Ok(())
}