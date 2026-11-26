pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;
use crate::remote::github::{push_to_github, resolve_github_token};
use crate::remote::transport::{not_a_repository, resolve_remote, types::Transport};
use crate::remote::push::engine::{
    collect_missing_objects,
    copy_object_to_remote,
    delete_remote_ref,
    delete_remote_tracking_ref,
    push_over_smart_http,
    read_remote_ref,
    set_upstream_branch,
    write_remote_ref,
    write_remote_tracking_ref,
};
use crate::core::commit::{is_reachable, resolve_commit_from_source, short_hash};
use crate::remote::{push::types::PushStatus, types::GitHubTarget};
use crate::core::refs::{self, HeadState};

pub fn push(remote: Option<String>, refspec: Option<String>, force: bool, set_upstream: bool) -> Result<()> {
    let head_state = refs::resolve_head()?;
    let current_branch_opt = match &head_state {
        HeadState::Branch(b) => Some(b.clone()),
        HeadState::Detached(_) => None,
    };

    let remote_name = match remote {
        Some(r) => r,
        None => {
            if let Some(ref b) = current_branch_opt {
                if let Some(r) = crate::core::config::get_config_value(&format!("branch.{}.remote", b))? {
                    r
                } else {
                    "origin".to_string()
                }
            } else {
                "origin".to_string()
            }
        }
    };

    let remote = resolve_remote(&remote_name)?;
    let (src_name, dest_branch) = parse_push_refspec(refspec, current_branch_opt.as_deref(), &remote_name)?;
    let remote_display = remote.display();
    let remote_git_dir = match remote.transport {
        Transport::GitHubApi { owner, repo } => {
            let token = resolve_github_token(&remote_name)?;
            let target = GitHubTarget { owner, repo, token };

            let local_commit_hash = if src_name.is_empty() {
                None
            } else if src_name == "HEAD" {
                Some(refs::resolve_head_commit()?.ok_or_else(|| anyhow::anyhow!("error: HEAD has no commits yet"))?)
            } else {
                Some(resolve_commit_from_source(&src_name)
                    .map_err(|_| anyhow::anyhow!("error: src refspec '{}' does not match any", src_name))?)
            };

            push_to_github(
                &target,
                &remote_name,
                &remote_display,
                &src_name,
                &dest_branch,
                local_commit_hash.as_deref(),
                force,
            )?;

            if set_upstream {
                set_upstream_after_push(current_branch_opt.as_deref(), &src_name, &remote_name, &dest_branch)?;
            }
            return Ok(());
        }
        Transport::Bundle { path } => return Err(not_a_repository(&path)),
        Transport::SmartHttp { url } => {
            push_over_smart_http(&url, &remote_name, &remote_display, &src_name, &dest_branch, force)?;
            if set_upstream && !src_name.is_empty() {
                set_upstream_after_push(current_branch_opt.as_deref(), &src_name, &remote_name, &dest_branch)?;
            }
            return Ok(());
        }
        Transport::Local { git_dir } => git_dir,
    };

    if src_name.is_empty() {
        delete_remote_ref(&remote_git_dir, &dest_branch)?;
        if !remote_name.contains('/') && !remote_name.contains('\\') {
            let _ = delete_remote_tracking_ref(&remote_name, &dest_branch);
        }
        println!("To {}", remote_display);
        println!(" - [deleted]         {}", dest_branch);
        return Ok(());
    }

    let local_commit_hash = if src_name == "HEAD" {
        refs::resolve_head_commit()?.ok_or_else(|| anyhow::anyhow!("error: HEAD has no commits yet"))?
    } else {
        resolve_commit_from_source(&src_name)
            .map_err(|_| anyhow::anyhow!("error: src refspec '{}' does not match any", src_name))?
    };

    let remote_commit_opt = read_remote_ref(&remote_git_dir, &dest_branch)?;
    let status = match remote_commit_opt {
        Some(ref remote_hash) => {
            if remote_hash == &local_commit_hash {
                println!("Everything up-to-date");
                return Ok(());
            }
            let is_ff = is_reachable(&local_commit_hash, remote_hash)?;
            if !is_ff && !force {
                eprintln!("To {}", remote_display);
                eprintln!(" ! [rejected]        {} -> {} (non-fast-forward)", src_name, dest_branch);
                anyhow::bail!(
                    "error: failed to push some refs to '{}'\n\
                     hint: Updates were rejected because the tip of your current branch is behind\n\
                     hint: its remote counterpart. Integrate the remote changes (e.g.\n\
                     hint: 'rgit merge') before pushing again.",
                    remote_name
                );
            }
            if is_ff {
                PushStatus::FastForward {
                    old_hash: remote_hash.clone(),
                    new_hash: local_commit_hash.clone(),
                }
            } else {
                PushStatus::ForcedUpdate {
                    old_hash: remote_hash.clone(),
                    new_hash: local_commit_hash.clone(),
                }
            }
        }
        None => PushStatus::NewBranch {
            new_hash: local_commit_hash.clone(),
        },
    };

    let stop_hash = match &status {
        PushStatus::FastForward { old_hash, .. } => Some(old_hash.as_str()),
        _ => None,
    };
    let missing = collect_missing_objects(&local_commit_hash, stop_hash, &remote_git_dir)?;
    let total = missing.len();
    println!("Enumerating objects: {}, done.", total);
    if total > 0 {
        let mut count = 0;
        for hash in &missing {
            copy_object_to_remote(hash, &remote_git_dir)?;
            count += 1;
        }
        println!("Writing objects: 100% ({}/{}), done.", count, total);
        println!("Total {} (delta 0), reused 0 (delta 0)", total);
    }

    write_remote_ref(&remote_git_dir, &dest_branch, &local_commit_hash)?;
    if !remote_name.contains('/') && !remote_name.contains('\\') {
        let _ = write_remote_tracking_ref(&remote_name, &dest_branch, &local_commit_hash);
    }
    println!("To {}", remote_display);
    match status {
        PushStatus::FastForward { old_hash, new_hash } => {
            println!(
                "   {}..{}  {} -> {}",
                short_hash(&old_hash),
                short_hash(&new_hash),
                src_name,
                dest_branch
            );
        }
        PushStatus::ForcedUpdate { old_hash, new_hash } => {
            println!(
                " + {}...{} {} -> {} (forced update)",
                short_hash(&old_hash),
                short_hash(&new_hash),
                src_name,
                dest_branch
            );
        }
        PushStatus::NewBranch { .. } => {
            println!(" * [new branch]      {} -> {}", src_name, dest_branch);
        }
    }

    if set_upstream {
        set_upstream_after_push(current_branch_opt.as_deref(), &src_name, &remote_name, &dest_branch)?;
    }

    Ok(())
}

/// Splits the push refspec into source and destination: `src:dest`, a single name for both,
/// or (none given) the current branch to the branch of the same name. An empty source
/// (`:dest`) deletes `dest`.
// Used by `push`.
fn parse_push_refspec(refspec: Option<String>, current_branch: Option<&str>, remote_name: &str) -> Result<(String, String)> {
    match refspec {
        Some(spec) => Ok(match spec.split_once(':') {
            Some((src, dest)) => (src.to_string(), dest.to_string()),
            None => (spec.clone(), spec),
        }),
        None => match current_branch {
            Some(branch) => Ok((branch.to_string(), branch.to_string())),
            None => anyhow::bail!(
                "fatal: You are not currently on a branch. To push the history leading to the current (detached HEAD) state now, use\n\n    rgit push {} HEAD:<name-of-remote-branch>",
                remote_name
            ),
        },
    }
}

/// `push -u`: makes the current branch track `<remote>/<dest>` when it was the branch pushed
/// (by name or as `HEAD`).
// Used by `push`.
fn set_upstream_after_push(current_branch: Option<&str>, src_name: &str, remote_name: &str, dest_branch: &str) -> Result<()> {
    if let Some(local_branch) = current_branch
        && (src_name == local_branch || src_name == "HEAD")
    {
        set_upstream_branch(local_branch, remote_name, dest_branch)?;
        println!("branch '{}' set up to track '{}/{}'.", local_branch, remote_name, dest_branch);
    }
    Ok(())
}
