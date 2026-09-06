use std::collections::BTreeMap;
use crate::helpers::objects::{flatten_tree, read_object, tree_hash_of_commit};
use crate::refs;

/// Resolves a commit's parent hashes from its raw object content, in order.
// Used by `cherry_pick`.
pub fn commit_parents(commit_text: &str) -> Vec<String> {
    commit_text
        .lines()
        .filter_map(|line| line.strip_prefix("parent "))
        .map(|s| s.to_string())
        .collect()
}

/// Extracts the full commit message body (everything after the blank line that
/// separates it from the header) from a commit object's raw content.
// Used by `cherry_pick`.
pub fn extract_commit_message(commit_text: &str) -> String {
    let mut in_message = false;
    let mut lines: Vec<&str> = Vec::new();
    for line in commit_text.lines() {
        if in_message {
            lines.push(line);
        } else if line.is_empty() {
            in_message = true;
        }
    }
    lines.join("\n")
}

/// Resolves a source branch, HEAD, commit hash, or parent expression (like `~N`)
/// into its commit hash.
// Used by `merge`, `diff`, `restore`, and tree resolution helpers.
pub fn resolve_commit_from_source(source: &str) -> anyhow::Result<String> {
    let (base_name, steps) = if let Some(idx) = source.find('~') {
        let base = &source[..idx];
        let num_str = &source[idx + 1..];
        let num: usize = num_str.parse().unwrap_or(1);
        (base, num)
    } else {
        (source, 0)
    };

    let mut commit_hash = if base_name.eq_ignore_ascii_case("head") {
        refs::resolve_head_commit()?.ok_or_else(|| {
            anyhow::anyhow!("error: could not resolve HEAD: HEAD has no commits yet")
        })?
    } else {
        let branch_ref = format!("refs/heads/{}", base_name);
        if let Some(hash) = refs::read_ref(&branch_ref)? {
            hash
        } else {
            match read_object(base_name) {
                Ok((ref obj_type, _)) if obj_type == "commit" => base_name.to_string(),
                Ok((obj_type, _)) => {
                    anyhow::bail!("error: '{}' is not a commit (it is a {})", base_name, obj_type);
                }
                Err(_) => {
                    anyhow::bail!("error: invalid reference: '{}'", base_name);
                }
            }
        }
    };

    for _ in 0..steps {
        let (obj_type, content) = read_object(&commit_hash)?;
        if obj_type != "commit" {
            anyhow::bail!("error: object {} is not a commit", commit_hash);
        }
        let text = String::from_utf8_lossy(&content);
        let parent = text.lines().find_map(|line| line.strip_prefix("parent ")).map(|s| s.to_string());
        if let Some(p) = parent {
            commit_hash = p;
        } else {
            anyhow::bail!("error: commit {} has no parent", commit_hash);
        }
    }

    Ok(commit_hash)
}

/// Resolves a source branch/commit name/hash into its tree structure, returning
/// a map of paths to their object hashes and modes.
// Used by `restore`, `diff`, and `merge` commands.
pub fn resolve_tree_from_source(source: &str) -> anyhow::Result<BTreeMap<String, ([u8; 20], u32)>> {
    let commit_hash = resolve_commit_from_source(source)?;
    let tree_hash = tree_hash_of_commit(&commit_hash)?;
    let mut tree_map = BTreeMap::new();
    flatten_tree(&tree_hash, "", &mut tree_map)?;
    Ok(tree_map)
}

/// Traverses parent commit hashes via BFS to check if the target commit is reachable
/// from the start commit.
// Used by `branch -d` safety checks to verify merge status.
pub fn is_reachable(start: &str, target: &str) -> anyhow::Result<bool> {
    use std::collections::{HashSet, VecDeque};

    if start == target {
        return Ok(true);
    }

    let mut visited: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back(start.to_string());

    while let Some(hash) = queue.pop_front() {
        if visited.contains(&hash) {
            continue;
        }
        visited.insert(hash.clone());

        if hash == target {
            return Ok(true);
        }

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }

        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent_hash) = line.strip_prefix("parent ") {
                queue.push_back(parent_hash.to_string());
            }
        }
    }

    Ok(false)
}

/// Returns the first line of a commit's message body (its "subject line").
// Used by `reset --hard` to print the familiar `HEAD is now at <hash> <subject>` line.
pub fn commit_subject_line(commit_hash: &str) -> anyhow::Result<String> {
    let (object_type, content) = read_object(commit_hash)?;
    if object_type != "commit" {
        anyhow::bail!("{} is not a commit object", commit_hash);
    }

    let text = String::from_utf8_lossy(&content);
    let mut in_message = false;
    for line in text.lines() {
        if in_message {
            return Ok(line.to_string());
        } else if line.is_empty() {
            in_message = true;
        }
    }

    Ok(String::new())
}