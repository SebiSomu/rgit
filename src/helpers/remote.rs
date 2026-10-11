use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use crate::helpers::config::{read_entries_from, write_entries_to, LOCAL_CONFIG_PATH};
use crate::objects::RemoteInfo;

/// Validates whether a given string is an acceptable remote name.
// Used by `add_remote` and `rename_remote`.
pub fn is_valid_remote_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.starts_with('-')
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains(':')
        && !name.contains('\n')
        && !name.contains(' ')
        && !name.contains('\t')
}

/// Checks if a remote with the specified name is configured in `.git/config`.
// Used by `remote` command and remote helpers.
pub fn remote_exists(name: &str) -> Result<bool> {
    let config_path = Path::new(LOCAL_CONFIG_PATH);
    let entries = read_entries_from(config_path)?;
    let prefix = format!("remote.{}.", name);
    Ok(entries.iter().any(|(k, _)| k.starts_with(&prefix)))
}

/// Returns a sorted list of all unique remote names defined in the repository's `.git/config`.
// Used by `remote` list and verbose commands.
pub fn list_remote_names() -> Result<Vec<String>> {
    let config_path = Path::new(LOCAL_CONFIG_PATH);
    let entries = read_entries_from(config_path)?;
    let mut names = BTreeSet::new();

    for (k, _) in entries {
        if let Some(rest) = k.strip_prefix("remote.") {
            if let Some(idx) = rest.rfind('.') {
                let remote_name = &rest[..idx];
                if !remote_name.is_empty() {
                    names.insert(remote_name.to_string());
                }
            }
        }
    }

    Ok(names.into_iter().collect())
}

/// Retrieves the configured fetch URL and push URL for a specified remote name.
// Used by `remote` verbose listing and `remote show`.
pub fn get_remote_urls(name: &str) -> Result<(Option<String>, Option<String>)> {
    let config_path = Path::new(LOCAL_CONFIG_PATH);
    let entries = read_entries_from(config_path)?;
    let url_key = format!("remote.{}.url", name);
    let push_key = format!("remote.{}.pushurl", name);

    let mut fetch_url = None;
    let mut push_url = None;

    for (k, v) in entries {
        if k == url_key {
            fetch_url = Some(v);
        } else if k == push_key {
            push_url = Some(v);
        }
    }

    if push_url.is_none() {
        push_url = fetch_url.clone();
    }

    Ok((fetch_url, push_url))
}

/// Adds a new remote with its repository URL and default fetch refspec to `.git/config`.
// Used by `rgit remote add`.
pub fn add_remote(name: &str, url: &str) -> Result<()> {
    if !is_valid_remote_name(name) {
        anyhow::bail!("fatal: '{}' is not a valid remote name", name);
    }
    if remote_exists(name)? {
        anyhow::bail!("fatal: remote {} already exists.", name);
    }

    let config_path = Path::new(LOCAL_CONFIG_PATH);
    let mut entries = read_entries_from(config_path)?;

    entries.push((format!("remote.{}.url", name), url.to_string()));
    entries.push((
        format!("remote.{}.fetch", name),
        format!("+refs/heads/*:refs/remotes/{}/*", name),
    ));

    write_entries_to(config_path, &entries)?;
    Ok(())
}

/// Removes a remote from `.git/config`, purges tracking branch refs, and removes its ref directory.
// Used by `rgit remote remove` / `rm`.
pub fn remove_remote(name: &str) -> Result<()> {
    if !remote_exists(name)? {
        anyhow::bail!("fatal: No such remote: '{}'", name);
    }

    let config_path = Path::new(LOCAL_CONFIG_PATH);
    let mut entries = read_entries_from(config_path)?;

    let remote_prefix = format!("remote.{}.", name);
    entries.retain(|(k, _)| !k.starts_with(&remote_prefix));

    let mut branches_tracking = Vec::new();
    for (k, v) in &entries {
        if let Some(branch_part) = k.strip_prefix("branch.") {
            if let Some(branch_name) = branch_part.strip_suffix(".remote") {
                if v == name {
                    branches_tracking.push(branch_name.to_string());
                }
            }
        }
    }

    for b in branches_tracking {
        let b_remote = format!("branch.{}.remote", b);
        let b_merge = format!("branch.{}.merge", b);
        entries.retain(|(k, _)| k != &b_remote && k != &b_merge);
    }

    write_entries_to(config_path, &entries)?;

    let remotes_dir = format!(".git/refs/remotes/{}", name);
    let remotes_path = Path::new(&remotes_dir);
    if remotes_path.exists() {
        fs::remove_dir_all(remotes_path)
            .with_context(|| format!("Failed to remove remote refs directory: {}", remotes_dir))?;
    }

    Ok(())
}

/// Renames a remote in `.git/config`, renames its ref directory, and updates branch tracking entries.
// Used by `rgit remote rename`.
pub fn rename_remote(old_name: &str, new_name: &str) -> Result<()> {
    if !is_valid_remote_name(new_name) {
        anyhow::bail!("fatal: '{}' is not a valid remote name", new_name);
    }
    if !remote_exists(old_name)? {
        anyhow::bail!("fatal: No such remote: '{}'", old_name);
    }
    if remote_exists(new_name)? {
        anyhow::bail!("fatal: remote {} already exists.", new_name);
    }

    let config_path = Path::new(LOCAL_CONFIG_PATH);
    let mut entries = read_entries_from(config_path)?;

    let old_prefix = format!("remote.{}.", old_name);
    let new_prefix = format!("remote.{}.", new_name);
    let old_fetch_ref = format!("refs/remotes/{}/*", old_name);
    let new_fetch_ref = format!("refs/remotes/{}/*", new_name);

    for (k, v) in entries.iter_mut() {
        if let Some(rest) = k.strip_prefix(&old_prefix) {
            let is_fetch = rest == "fetch";
            let new_key = format!("{}{}", new_prefix, rest);
            *k = new_key;
            if is_fetch && v.contains(&old_fetch_ref) {
                *v = v.replace(&old_fetch_ref, &new_fetch_ref);
            }
        } else if k.starts_with("branch.") && k.ends_with(".remote") && v == old_name {
            *v = new_name.to_string();
        }
    }

    write_entries_to(config_path, &entries)?;

    let old_path = format!(".git/refs/remotes/{}", old_name);
    let new_path = format!(".git/refs/remotes/{}", new_name);
    if Path::new(&old_path).exists() {
        if let Some(parent) = Path::new(&new_path).parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&old_path, &new_path)
            .with_context(|| format!("Failed to rename remote directory {} to {}", old_path, new_path))?;
    }

    Ok(())
}

/// Retrieves detailed information about a remote including URLs and tracking branches.
// Used by `rgit remote show`.
pub fn get_remote_info(name: &str) -> Result<RemoteInfo> {
    if !remote_exists(name)? {
        anyhow::bail!("fatal: No such remote: '{}'", name);
    }

    let (fetch_url, push_url) = get_remote_urls(name)?;
    let mut tracking_branches = Vec::new();

    let remotes_dir = format!(".git/refs/remotes/{}", name);
    if Path::new(&remotes_dir).exists() {
        collect_remote_branches(Path::new(&remotes_dir), "", &mut tracking_branches)?;
        tracking_branches.sort();
    }

    Ok(RemoteInfo {
        name: name.to_string(),
        fetch_url,
        push_url,
        tracking_branches,
    })
}

/// Recursively walks `.git/refs/remotes/<remote>/` to find all tracked branches.
// Used by `get_remote_info`.
fn collect_remote_branches(dir: &Path, prefix: &str, out: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().to_string();
        let full_name = if prefix.is_empty() {
            file_name.clone()
        } else {
            format!("{}/{}", prefix, file_name)
        };

        if entry.metadata()?.is_dir() {
            collect_remote_branches(&entry.path(), &full_name, out)?;
        } else if file_name != "HEAD" {
            out.push(full_name);
        }
    }
    Ok(())
}
