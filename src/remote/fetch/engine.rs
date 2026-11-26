use anyhow::{Context, Result};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::Path;

/// Walks a remote repository's `refs/heads/` directory (and its `packed-refs`) and returns
/// all branch names paired with their commit SHA strings.
// Used by `fetch`.
pub fn list_remote_refs(remote_git_dir: &Path) -> Result<Vec<(String, String)>> {
    let heads_dir = remote_git_dir.join("refs").join("heads");
    let mut refs = Vec::new();
    if heads_dir.exists() {
        collect_refs_recursive(&heads_dir, &heads_dir, &mut refs)?;
    }
    // Branches the remote keeps in `packed-refs` (a loose ref of the same name wins).
    for (branch, sha) in remote_packed_heads(remote_git_dir) {
        if !refs.iter().any(|(name, _)| *name == branch) {
            refs.push((branch, sha));
        }
    }
    // By name, as git lists refs: directory order differs between file systems.
    refs.sort();
    Ok(refs)
}

/// The branches listed in a remote repository's `packed-refs` file, as `(name, sha)`.
// Used by `list_remote_refs` and `push` (`read_remote_ref`).
pub fn remote_packed_heads(remote_git_dir: &Path) -> Vec<(String, String)> {
    let Ok(packed) = fs::read_to_string(remote_git_dir.join("packed-refs")) else { return Vec::new() };
    packed
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .filter_map(|line| line.split_once(' '))
        .filter_map(|(sha, name)| name.strip_prefix("refs/heads/").map(|branch| (branch.to_string(), sha.to_string())))
        .filter(|(_, sha)| sha.len() == 40)
        .collect()
}

/// Recursive helper that walks subdirectories under `refs/heads/` to support nested branch names.
// Used by `fetch`.
fn collect_refs_recursive(base: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("Cannot read refs dir: {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_refs_recursive(base, &path, out)?;
        } else {
            let rel = path.strip_prefix(base).unwrap_or(&path);
            let branch_name = rel.to_string_lossy().replace('\\', "/");
            let sha = fs::read_to_string(&path)
                .with_context(|| format!("Cannot read ref: {}", path.display()))?
                .trim()
                .to_string();
            if !sha.is_empty() {
                out.push((branch_name, sha));
            }
        }
    }
    Ok(())
}

/// Reads the remote repository's HEAD file and returns the branch name it points to,
/// or None if HEAD is detached.
// Used by `fetch`.
pub fn read_remote_head(remote_git_dir: &Path) -> Result<Option<String>> {
    let head_path = remote_git_dir.join("HEAD");
    if !head_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&head_path).context("Cannot read remote HEAD")?;
    let trimmed = content.trim();
    if let Some(rest) = trimmed.strip_prefix("ref: refs/heads/") {
        Ok(Some(rest.to_string()))
    } else {
        Ok(None)
    }
}

/// Checks whether a git object with the given SHA exists in the local repository.
// Used by `fetch`.
pub fn local_has_object(hash: &str) -> bool {
    if hash.len() < 3 {
        return false;
    }
    crate::core::repo::path(format!(".git/objects/{}/{}", &hash[..2], &hash[2..])).exists() || crate::core::pack::packed_object_exists(hash)
}

/// Checks that a fetched pack is complete on top of this repository: every object a commit,
/// tree or tag in `objects` references is either in the pack (`in_pack`) or already here.
/// Submodule commits (gitlinks) are not objects of this repository and are skipped. Returns
/// the first referencing object and the missing name, or `None` when nothing is missing.
// Used by `fetch` over HTTPS.
pub fn first_missing_reference(objects: &[crate::core::types::PackedObject], in_pack: &HashSet<&str>) -> Option<(String, String)> {
    let mut checked: HashSet<String> = HashSet::new();
    for object in objects {
        for reference in object_references(&object.kind, &object.content) {
            if in_pack.contains(reference.as_str()) || checked.contains(&reference) {
                continue;
            }
            if !local_has_object(&reference) {
                return Some((object.hash.clone(), reference));
            }
            checked.insert(reference);
        }
    }
    None
}

/// The objects a commit (tree, parents), tree (entries other than gitlinks) or tag (target)
/// refers to; nothing for a blob or a malformed object.
// Used by `first_missing_reference`.
pub fn object_references(kind: &str, content: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    match kind {
        "commit" | "tag" => {
            let text = String::from_utf8_lossy(content);
            for line in text.split('\n').take_while(|l| !l.is_empty()) {
                let name = match kind {
                    "commit" => line.strip_prefix("tree ").or_else(|| line.strip_prefix("parent ")),
                    _ => line.strip_prefix("object "),
                };
                if let Some(name) = name.map(str::trim).filter(|n| n.len() == 40) {
                    out.push(name.to_string());
                }
            }
        }
        "tree" => {
            let mut rest = content;
            while let Some(nul) = rest.iter().position(|&b| b == 0) {
                if rest.len() < nul + 21 {
                    break;
                }
                let gitlink = rest.starts_with(b"160000 ");
                if !gitlink {
                    out.push(hex::encode(&rest[nul + 1..nul + 21]));
                }
                rest = &rest[nul + 21..];
            }
        }
        _ => {}
    }
    out
}

/// BFS-walks a commit graph from `remote_commit` using the remote's object store and
/// collects all commit, tree, and blob hashes that are missing from the local repository.
// Used by `fetch`.
pub fn enumerate_objects_to_fetch(remote_commit: &str, remote_git_dir: &Path) -> Result<HashSet<String>> {
    let mut to_fetch = HashSet::new();
    let mut visited_commits = HashSet::new();
    let mut visited_trees = HashSet::new();
    let mut queue = VecDeque::new();

    queue.push_back(remote_commit.to_string());

    while let Some(commit_hash) = queue.pop_front() {
        if !visited_commits.insert(commit_hash.clone()) {
            continue;
        }
        if local_has_object(&commit_hash) {
            continue;
        }

        to_fetch.insert(commit_hash.clone());

        let obj_path = remote_git_dir
            .join("objects")
            .join(&commit_hash[..2])
            .join(&commit_hash[2..]);

        let (obj_type, content) = read_object_from_path(&obj_path)
            .with_context(|| format!("Failed to read remote object {}", commit_hash))?;

        if obj_type != "commit" {
            anyhow::bail!("{} is not a commit object", commit_hash);
        }

        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(tree_hash) = line.strip_prefix("tree ") {
                collect_tree_objects_to_fetch(tree_hash, remote_git_dir, &mut to_fetch, &mut visited_trees)?;
            } else if let Some(parent_hash) = line.strip_prefix("parent ") {
                queue.push_back(parent_hash.to_string());
            }
        }
    }

    Ok(to_fetch)
}

/// Recursively collects all tree and blob hashes from a remote tree object that are
/// missing locally.
// Used by `fetch`.
fn collect_tree_objects_to_fetch(tree_hash: &str, remote_git_dir: &Path, to_fetch: &mut HashSet<String>, visited_trees: &mut HashSet<String>) -> Result<()> {
    if !visited_trees.insert(tree_hash.to_string()) {
        return Ok(());
    }
    if local_has_object(tree_hash) {
        return Ok(());
    }
    to_fetch.insert(tree_hash.to_string());

    let obj_path = remote_git_dir
        .join("objects")
        .join(&tree_hash[..2])
        .join(&tree_hash[2..]);

    let (obj_type, content) = read_object_from_path(&obj_path)
        .with_context(|| format!("Failed to read remote tree {}", tree_hash))?;

    if obj_type != "tree" {
        anyhow::bail!("{} is not a tree object", tree_hash);
    }

    let mut pos = 0;
    while pos < content.len() {
        let space_pos = content[pos..]
            .iter()
            .position(|&b| b == b' ')
            .context("Missing mode separator in tree")?
            + pos;
        let mode = String::from_utf8_lossy(&content[pos..space_pos]);

        let null_pos = content[space_pos..]
            .iter()
            .position(|&b| b == 0)
            .context("Missing name terminator in tree")?
            + space_pos;

        let hash_start = null_pos + 1;
        let hash_end = hash_start + 20;
        if hash_end > content.len() {
            anyhow::bail!("Corrupted tree object");
        }
        let sha_hex = hex::encode(&content[hash_start..hash_end]);

        if mode == "160000" {
            // A submodule link names a commit of another repository: nothing to fetch.
        } else if mode == "40000" || mode == "040000" {
            collect_tree_objects_to_fetch(&sha_hex, remote_git_dir, to_fetch, visited_trees)?;
        } else if !local_has_object(&sha_hex) {
            to_fetch.insert(sha_hex);
        }

        pos = hash_end;
    }

    Ok(())
}

/// Reads and decompresses a zlib-compressed git object from an explicit filesystem path,
/// returning its type and raw content bytes.
// Used by `fetch`.
pub fn read_object_from_path(path: &Path) -> Result<(String, Vec<u8>)> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    // Not a loose file: `<objects>/<xx>/<38 hex>` may still be an object in one of that
    // repository's packs.
    if !path.exists() {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let prefix = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if let Some(objects_dir) = path.parent().and_then(|p| p.parent()) {
            if let Some(object) = crate::core::pack::read_object_in_objects_dir(objects_dir, &format!("{}{}", prefix, name)) {
                return Ok(object);
            }
        }
    }
    let compressed = fs::read(path)
        .with_context(|| format!("Cannot read object at {}", path.display()))?;

    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed).context("Failed to decompress git object")?;

    let null_pos = decompressed
        .iter()
        .position(|&b| b == 0)
        .context("Invalid git object: missing null byte")?;

    let header = String::from_utf8_lossy(&decompressed[..null_pos]);
    let obj_type = header
        .split(' ')
        .next()
        .context("Invalid git object header")?
        .to_string();

    Ok((obj_type, decompressed[null_pos + 1..].to_vec()))
}

/// Copies a single zlib-compressed object file from a remote `.git/objects/` directory
/// to the local `.git/objects/` directory.
// Used by `fetch`.
pub fn fetch_object_from_remote(hash: &str, remote_git_dir: &Path) -> Result<()> {
    if hash.len() < 3 {
        anyhow::bail!("Invalid object hash: {}", hash);
    }
    let remote_path = remote_git_dir
        .join("objects")
        .join(&hash[..2])
        .join(&hash[2..]);
    let local_dir = crate::core::repo::path(format!(".git/objects/{}", &hash[..2]));
    let local_path = local_dir.join(&hash[2..]);

    fs::create_dir_all(&local_dir)?;
    if !local_path.exists() {
        if remote_path.exists() {
            fs::copy(&remote_path, &local_path)
                .with_context(|| format!("Failed to copy object {} from remote", hash))?;
        } else {
            // The remote keeps it in a pack: read it there and store it loose here.
            let (kind, content) = crate::core::pack::read_object_in_objects_dir(&remote_git_dir.join("objects"), hash)
                .with_context(|| format!("Failed to copy object {} from remote", hash))?;
            crate::core::object_store::write_object(&kind, &content)?;
        }
    }
    Ok(())
}

/// Reads the current SHA stored in a local remote-tracking ref file, returning None if it
/// doesn't exist yet.
// Used by `fetch`.
pub fn read_local_remote_tracking_ref(remote_name: &str, branch_name: &str) -> Option<String> {
    let path = crate::core::repo::path(format!(".git/refs/remotes/{}/{}", remote_name, branch_name));
    fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Writes a local remote-tracking ref (`refs/remotes/<remote>/<branch>`) to point to a
/// new commit SHA.
// Used by `fetch`.
pub fn write_local_remote_tracking_ref(remote_name: &str, branch_name: &str, sha: &str) -> Result<()> {
    let ref_path = crate::core::repo::path(format!(".git/refs/remotes/{}/{}", remote_name, branch_name));
    let path = ref_path.as_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", sha))
        .with_context(|| format!("Failed to write remote-tracking ref {}/{}", remote_name, branch_name))?;
    Ok(())
}

/// Writes `.git/FETCH_HEAD` with one line per fetched branch, recording the SHA and
/// display URL for use by `merge FETCH_HEAD`.
// Used by `fetch`.
pub fn write_fetch_head(entries: &[(String, String, String)]) -> Result<()> {
    // Each entry: (sha, branch_name, remote_url)
    let mut lines = Vec::new();
    for (i, (sha, branch, remote_url)) in entries.iter().enumerate() {
        let merge_flag = if i == 0 { "" } else { "\tnot-for-merge" };
        lines.push(format!("{}\t{}branch '{}' of {}", sha, merge_flag, branch, remote_url));
    }
    fs::write(crate::core::repo::path(".git/FETCH_HEAD"), lines.join("\n") + "\n")
        .context("Failed to write FETCH_HEAD")?;
    Ok(())
}

