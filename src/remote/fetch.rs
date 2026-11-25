pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;
use crate::remote::fetch::engine::{
    enumerate_objects_to_fetch,
    fetch_object_from_remote,
    list_remote_refs,
    read_local_remote_tracking_ref,
    read_remote_head,
    write_fetch_head,
    write_local_remote_tracking_ref,
};
use crate::remote::bundle::engine::{bundle_branches, bundle_head_branch, check_bundle_prerequisites, is_bundle_file, read_bundle, read_packfile, store_loose_objects};
use crate::remote::github::{fetch_from_github, parse_github_remote, resolve_github_token};
use crate::remote::push::engine::resolve_remote_repo_path;
use crate::remote::{fetch::types::FetchRefResult, types::GitHubTarget};

pub fn fetch(remote: Option<String>, branch: Option<String>) -> Result<()> {
    let remote_name = match remote {
        Some(r) => r,
        None => {
            if let Ok(Some(b)) = crate::core::refs::resolve_head()
                .map(|h| match h {
                    crate::core::refs::HeadState::Branch(b) => Some(b),
                    _ => None,
                })
            {
                crate::core::config::get_config_value(&format!("branch.{}.remote", b))?
                    .unwrap_or_else(|| "origin".to_string())
            } else {
                "origin".to_string()
            }
        }
    };

    let configured_url =
        crate::core::config::get_config_value(&format!("remote.{}.url", remote_name))?;

    let github_info = configured_url
        .as_deref()
        .and_then(parse_github_remote)
        .or_else(|| parse_github_remote(&remote_name));

    if let Some((owner, repo)) = github_info {
        let remote_display = configured_url
            .unwrap_or_else(|| format!("https://github.com/{}/{}", owner, repo));
        let token = resolve_github_token(&remote_name)?;
        let target = GitHubTarget { owner, repo, token };

        fetch_from_github(&target, &remote_name, &remote_display, branch.as_deref())?;
        return Ok(());
    }

    let bundle_path = std::path::PathBuf::from(configured_url.as_deref().unwrap_or(&remote_name));
    if is_bundle_file(&bundle_path) {
        let remote_display = configured_url.unwrap_or_else(|| remote_name.clone());
        return fetch_from_bundle(&bundle_path, &remote_name, &remote_display, branch.as_deref());
    }

    let remote_git_dir = resolve_remote_repo_path(&remote_name)?;
    let remote_display = configured_url
        .unwrap_or_else(|| remote_git_dir.display().to_string());

    let remote_refs = list_remote_refs(&remote_git_dir)?;
    let filtered_refs: Vec<_> = match &branch {
        Some(b) => remote_refs
            .into_iter()
            .filter(|(name, _)| name == b)
            .collect(),
        None => remote_refs,
    };

    if filtered_refs.is_empty() {
        println!("From {}", remote_display);
        println!(" * (no branches found or empty repository)");
        return Ok(());
    }

    println!("From {}", remote_display);

    let mut total_objects = 0usize;
    let mut results: Vec<FetchRefResult> = Vec::new();
    let mut fetch_head_entries = Vec::new();

    for (branch_name, remote_sha) in &filtered_refs {
        let local_sha =
            read_local_remote_tracking_ref(&remote_name, branch_name);

        if local_sha.as_deref() == Some(remote_sha.as_str()) {
            println!(
                " = [up to date]      {:20} -> {}/{}",
                branch_name, remote_name, branch_name
            );
            results.push(FetchRefResult::UpToDate {
                branch: branch_name.clone(),
            });
            fetch_head_entries.push((
                remote_sha.clone(),
                branch_name.clone(),
                remote_display.clone(),
            ));
            continue;
        }

        let missing = enumerate_objects_to_fetch(remote_sha, &remote_git_dir)?;
        let count = missing.len();
        total_objects += count;

        for hash in &missing {
            fetch_object_from_remote(hash, &remote_git_dir)?;
        }

        write_local_remote_tracking_ref(&remote_name, branch_name, remote_sha)?;
        fetch_head_entries.push((
            remote_sha.clone(),
            branch_name.clone(),
            remote_display.clone(),
        ));

        let result = match &local_sha {
            None => {
                println!(
                    " * [new branch]      {:20} -> {}/{}",
                    branch_name, remote_name, branch_name
                );
                FetchRefResult::NewBranch {
                    branch: branch_name.clone(),
                    sha: remote_sha.clone(),
                }
            }
            Some(old) => {
                println!(
                    "   {}..{}  {:16} -> {}/{}",
                    crate::core::commit::short_hash(old),
                    crate::core::commit::short_hash(remote_sha),
                    branch_name, remote_name, branch_name
                );
                FetchRefResult::Updated {
                    branch: branch_name.clone(),
                    old_sha: old.clone(),
                    new_sha: remote_sha.clone(),
                }
            }
        };
        results.push(result);
    }

    if total_objects > 0 {
        println!(
            "Receiving objects: 100% ({}/{}), done.",
            total_objects, total_objects
        );
    }

    if let Ok(Some(default_branch)) = read_remote_head(&remote_git_dir) {
        if let Some((_, sha)) = filtered_refs.iter().find(|(b, _)| b == &default_branch) {
            let _ = crate::core::refs::write_remote_head(&remote_name, &default_branch, sha);
        }
    }

    write_fetch_head(&fetch_head_entries)?;

    Ok(())
}


/// `rgit fetch <bundle>` (or a remote whose url is a bundle file): checks the bundle's
/// prerequisites, stores its objects here, then updates `refs/remotes/<remote>/<branch>` for
/// each branch it carries (only `branch` when given), its remote HEAD and FETCH_HEAD, with
/// the same output as a fetch from a local repository.
fn fetch_from_bundle(path: &std::path::Path, remote_name: &str, remote_display: &str, branch: Option<&str>) -> Result<()> {
    let (header, bytes) = read_bundle(&path.to_string_lossy())?;
    check_bundle_prerequisites(&header)?;
    let branches: Vec<(String, String)> = bundle_branches(&header).into_iter().filter(|(name, _)| branch.is_none_or(|b| b == name)).collect();

    println!("From {}", remote_display);
    if branches.is_empty() {
        println!(" * (no branches found or empty repository)");
        return Ok(());
    }

    let mut received = 0usize;
    let mut fetch_head_entries = Vec::new();
    for (branch_name, remote_sha) in &branches {
        fetch_head_entries.push((remote_sha.clone(), branch_name.clone(), remote_display.to_string()));
        let local_sha = read_local_remote_tracking_ref(remote_name, branch_name);
        if local_sha.as_deref() == Some(remote_sha.as_str()) {
            println!(" = [up to date]      {:20} -> {}/{}", branch_name, remote_name, branch_name);
            continue;
        }
        // The objects are stored once, before the first ref that needs them.
        if received == 0 {
            received = store_loose_objects(&read_packfile(&bytes[header.pack_offset..])?)?;
        }
        write_local_remote_tracking_ref(remote_name, branch_name, remote_sha)?;
        match &local_sha {
            None => println!(" * [new branch]      {:20} -> {}/{}", branch_name, remote_name, branch_name),
            Some(old) => println!(
                "   {}..{}  {:16} -> {}/{}",
                crate::core::commit::short_hash(old),
                crate::core::commit::short_hash(remote_sha),
                branch_name, remote_name, branch_name
            ),
        }
    }

    if received > 0 {
        println!("Receiving objects: 100% ({}/{}), done.", received, received);
    }
    if let Some(default_branch) = bundle_head_branch(&header)
        && let Some((_, sha)) = branches.iter().find(|(b, _)| *b == default_branch)
    {
        let _ = crate::core::refs::write_remote_head(remote_name, &default_branch, sha);
    }
    write_fetch_head(&fetch_head_entries)?;
    Ok(())
}
