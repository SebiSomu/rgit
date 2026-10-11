use anyhow::Result;
use crate::helpers::fetch::*;
use crate::helpers::github::{fetch_from_github, parse_github_remote, resolve_github_token};
use crate::helpers::push::resolve_remote_repo_path;
use crate::objects::{FetchRefResult, GitHubTarget};

pub fn fetch(remote: Option<String>, branch: Option<String>) -> Result<()> {
    let remote_name = match remote {
        Some(r) => r,
        None => {
            if let Ok(Some(b)) = crate::refs::resolve_head()
                .map(|h| match h {
                    crate::refs::HeadState::Branch(b) => Some(b),
                    _ => None,
                })
            {
                crate::helpers::config::get_config_value(&format!("branch.{}.remote", b))?
                    .unwrap_or_else(|| "origin".to_string())
            } else {
                "origin".to_string()
            }
        }
    };

    let configured_url =
        crate::helpers::config::get_config_value(&format!("remote.{}.url", remote_name))?;

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
                    crate::helpers::short_hash(old),
                    crate::helpers::short_hash(remote_sha),
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
            let _ = crate::refs::write_ref(
                &format!("refs/remotes/{}/HEAD", remote_name),
                sha,
            );
        }
    }

    write_fetch_head(&fetch_head_entries)?;

    Ok(())
}
