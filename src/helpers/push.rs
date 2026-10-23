use anyhow::{Context, Result};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

/// Resolves the remote specification (a remote name in config or a direct filesystem path)
/// into the remote repository's `.git/` (or bare) directory.
// Used by `push`.
pub fn resolve_remote_repo_path(remote_spec: &str) -> Result<PathBuf> {
    let resolved_path = if let Some(url) = crate::helpers::config::get_config_value(&format!("remote.{}.url", remote_spec))? {
        PathBuf::from(url)
    } else {
        let p = PathBuf::from(remote_spec);
        if p.exists() {
            p
        } else {
            anyhow::bail!(
                "fatal: '{}' does not appear to be a git repository\nfatal: Could not read from remote repository.",
                remote_spec
            );
        }
    };

    if resolved_path.join(".git").is_dir() {
        Ok(resolved_path.join(".git"))
    } else if resolved_path.join("objects").is_dir() && (resolved_path.join("refs").is_dir() || resolved_path.join("HEAD").exists()) {
        Ok(resolved_path)
    } else if resolved_path.exists() {
        anyhow::bail!(
            "fatal: '{}' does not appear to be a git repository",
            resolved_path.display()
        );
    } else {
        anyhow::bail!(
            "fatal: '{}' does not exist",
            resolved_path.display()
        );
    }
}

/// Checks whether an object with the given hexadecimal SHA-1 exists in the remote repository.
// Used by `push`.
pub fn remote_has_object(remote_git_dir: &Path, hash: &str) -> bool {
    if hash.len() < 3 {
        return false;
    }
    remote_git_dir
        .join("objects")
        .join(&hash[..2])
        .join(&hash[2..])
        .exists()
}

/// Recursively inspects a tree object and collects all reachable tree and blob hashes
/// that are missing in the remote repository.
// Used by `push`.
pub fn collect_tree_objects(tree_hash: &str, remote_git_dir: &Path, to_copy: &mut HashSet<String>, visited_trees: &mut HashSet<String>) -> Result<()> {
    if !visited_trees.insert(tree_hash.to_string()) {
        return Ok(());
    }
    if remote_has_object(remote_git_dir, tree_hash) {
        return Ok(());
    }
    to_copy.insert(tree_hash.to_string());

    let (object_type, content) = crate::helpers::read_object(tree_hash)?;
    if object_type != "tree" {
        anyhow::bail!("{} is not a tree object", tree_hash);
    }

    let mut pos = 0;
    while pos < content.len() {
        let space_pos = content[pos..]
            .iter()
            .position(|&b| b == b' ')
            .context("Missing mode separator in tree object")?
            + pos;
        let mode = String::from_utf8_lossy(&content[pos..space_pos]);

        let null_pos = content[space_pos..]
            .iter()
            .position(|&b| b == 0)
            .context("Missing name terminator in tree object")?
            + space_pos;

        let hash_start = null_pos + 1;
        let hash_end = hash_start + 20;
        if hash_end > content.len() {
            anyhow::bail!("Corrupted tree object: incomplete hash");
        }
        let sha_hex = hex::encode(&content[hash_start..hash_end]);

        if mode == "40000" || mode == "040000" {
            collect_tree_objects(&sha_hex, remote_git_dir, to_copy, visited_trees)?;
        } else if !remote_has_object(remote_git_dir, &sha_hex) {
            to_copy.insert(sha_hex);
        }

        pos = hash_end;
    }

    Ok(())
}

/// Walks the commit graph from `local_tip` down to root or `stop_hash`, collecting
/// all commit, tree, and blob hashes missing in the remote repository.
// Used by `push`.
pub fn collect_missing_objects(local_tip: &str, stop_hash: Option<&str>, remote_git_dir: &Path) -> Result<HashSet<String>> {
    let mut to_copy = HashSet::new();
    let mut visited_commits = HashSet::new();
    let mut visited_trees = HashSet::new();
    let mut queue = VecDeque::new();

    queue.push_back(local_tip.to_string());

    while let Some(commit_hash) = queue.pop_front() {
        if !visited_commits.insert(commit_hash.clone()) {
            continue;
        }
        if Some(commit_hash.as_str()) == stop_hash {
            continue;
        }
        if remote_has_object(remote_git_dir, &commit_hash) {
            continue;
        }

        to_copy.insert(commit_hash.clone());

        let (object_type, content) = crate::helpers::read_object(&commit_hash)?;
        if object_type != "commit" {
            anyhow::bail!("{} is not a commit object", commit_hash);
        }

        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(tree_hash) = line.strip_prefix("tree ") {
                collect_tree_objects(tree_hash, remote_git_dir, &mut to_copy, &mut visited_trees)?;
            } else if let Some(parent_hash) = line.strip_prefix("parent ") {
                if Some(parent_hash) != stop_hash {
                    queue.push_back(parent_hash.to_string());
                }
            }
        }
    }

    Ok(to_copy)
}

/// Copies a zlib-compressed object file from the local repository to the remote repository.
// Used by `push`.
pub fn copy_object_to_remote(hash: &str, remote_git_dir: &Path) -> Result<()> {
    if hash.len() < 3 {
        anyhow::bail!("Invalid object hash: {}", hash);
    }
    let local_path = crate::repo::path(format!(".git/objects/{}/{}", &hash[..2], &hash[2..]));
    let remote_dir = remote_git_dir.join("objects").join(&hash[..2]);
    let remote_path = remote_dir.join(&hash[2..]);

    fs::create_dir_all(&remote_dir)?;
    if !remote_path.exists() {
        fs::copy(&local_path, &remote_path)
            .with_context(|| format!("Failed to copy object {} to remote", hash))?;
    }
    Ok(())
}

/// Reads the commit hash pointed to by a branch reference in the remote repository.
// Used by `push`.
pub fn read_remote_ref(remote_git_dir: &Path, branch_name: &str) -> Result<Option<String>> {
    let path = remote_git_dir.join("refs").join("heads").join(branch_name);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read remote ref: {}", branch_name))?;
    Ok(Some(content.trim().to_string()))
}

/// Writes a commit hash to a branch reference in the remote repository.
// Used by `push`.
pub fn write_remote_ref(remote_git_dir: &Path, branch_name: &str, hash: &str) -> Result<()> {
    let path = remote_git_dir.join("refs").join("heads").join(branch_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, format!("{}\n", hash))
        .with_context(|| format!("Failed to write remote ref: {}", branch_name))?;
    Ok(())
}

/// Deletes a branch reference file in the remote repository and removes empty parent directories.
// Used by `push`.
pub fn delete_remote_ref(remote_git_dir: &Path, branch_name: &str) -> Result<()> {
    let path = remote_git_dir.join("refs").join("heads").join(branch_name);
    if !path.exists() {
        anyhow::bail!("error: unable to delete '{}': remote ref does not exist", branch_name);
    }
    fs::remove_file(&path)
        .with_context(|| format!("Failed to delete remote ref '{}'", branch_name))?;

    let heads_root = remote_git_dir.join("refs").join("heads");
    let mut parent = path.parent();
    while let Some(p) = parent {
        if p == heads_root || p == remote_git_dir.join("refs") {
            break;
        }
        let is_empty = fs::read_dir(p)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = fs::remove_dir(p);
        } else {
            break;
        }
        parent = p.parent();
    }
    Ok(())
}

/// Updates the local remote-tracking reference (e.g. `refs/remotes/origin/main`) to match the pushed commit.
// Used by `push`.
pub fn write_remote_tracking_ref(remote_name: &str, branch_name: &str, hash: &str) -> Result<()> {
    let ref_name = format!("refs/remotes/{}/{}", remote_name, branch_name);
    crate::refs::write_ref(&ref_name, hash)?;
    Ok(())
}

/// Deletes a local remote-tracking reference if it exists.
// Used by `push`.
pub fn delete_remote_tracking_ref(remote_name: &str, branch_name: &str) -> Result<()> {
    let ref_name = format!("refs/remotes/{}/{}", remote_name, branch_name);
    let path = crate::repo::path(format!(".git/{}", ref_name));
    if Path::new(&path).exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Configures upstream tracking for a local branch in `.git/config`.
// Used by `push`.
pub fn set_upstream_branch(local_branch: &str, remote_name: &str, remote_branch: &str) -> Result<()> {
    let config_path = &crate::repo::path(crate::helpers::config::LOCAL_CONFIG_PATH);
    crate::helpers::config::set_in(config_path, &format!("branch.{}.remote", local_branch), remote_name)?;
    crate::helpers::config::set_in(config_path, &format!("branch.{}.merge", local_branch), &format!("refs/heads/{}", remote_branch))?;
    Ok(())
}
