use anyhow::{Context, Result};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

/// Resolves the remote specification (a remote name in config or a direct filesystem path)
/// into the remote repository's `.git/` (or bare) directory.
// Used by `push`.
pub fn resolve_remote_repo_path(remote_spec: &str) -> Result<PathBuf> {
    let resolved_path = if let Some(url) = crate::core::config::get_config_value(&format!("remote.{}.url", remote_spec))? {
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
    // Loose there, or in one of the remote's packs.
    remote_git_dir.join("objects").join(&hash[..2]).join(&hash[2..]).exists()
        || crate::core::pack::read_object_in_objects_dir(&remote_git_dir.join("objects"), hash).is_some()
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

    let (object_type, content) = crate::core::object_store::read_object(tree_hash)?;
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

        if mode == "160000" {
            // A submodule link names a commit of another repository: nothing to copy.
        } else if mode == "40000" || mode == "040000" {
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

        let (object_type, content) = crate::core::object_store::read_object(&commit_hash)?;
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
    let local_path = crate::core::repo::path(format!(".git/objects/{}/{}", &hash[..2], &hash[2..]));
    let remote_dir = remote_git_dir.join("objects").join(&hash[..2]);
    let remote_path = remote_dir.join(&hash[2..]);

    fs::create_dir_all(&remote_dir)?;
    if !remote_path.exists() {
        if local_path.exists() {
            fs::copy(&local_path, &remote_path)
                .with_context(|| format!("Failed to copy object {} to remote", hash))?;
        } else {
            // Packed here: read it and store it loose in the remote.
            let (kind, content) = crate::core::object_store::read_object(hash).with_context(|| format!("Failed to copy object {} to remote", hash))?;
            crate::core::pack::write_loose_object_in(&remote_git_dir.join("objects"), &kind, &content)?;
        }
    }
    Ok(())
}

/// Reads the commit hash pointed to by a branch reference in the remote repository.
// Used by `push`.
pub fn read_remote_ref(remote_git_dir: &Path, branch_name: &str) -> Result<Option<String>> {
    let path = remote_git_dir.join("refs").join("heads").join(branch_name);
    if !path.exists() {
        // The remote may keep the branch in `packed-refs`.
        return Ok(crate::remote::fetch::engine::remote_packed_heads(remote_git_dir).into_iter().find(|(name, _)| name == branch_name).map(|(_, sha)| sha));
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
    crate::core::refs::write_ref(&ref_name, hash)?;
    Ok(())
}

/// Deletes a local remote-tracking reference if it exists.
// Used by `push`.
pub fn delete_remote_tracking_ref(remote_name: &str, branch_name: &str) -> Result<()> {
    let ref_name = format!("refs/remotes/{}/{}", remote_name, branch_name);
    let path = crate::core::repo::path(format!(".git/{}", ref_name));
    if Path::new(&path).exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// Configures upstream tracking for a local branch in `.git/config`.
// Used by `push`.
pub fn set_upstream_branch(local_branch: &str, remote_name: &str, remote_branch: &str) -> Result<()> {
    let config_path = &crate::core::repo::path(crate::core::config::LOCAL_CONFIG_PATH);
    crate::core::config::set_in(config_path, &format!("branch.{}.remote", local_branch), remote_name)?;
    crate::core::config::set_in(config_path, &format!("branch.{}.merge", local_branch), &format!("refs/heads/{}", remote_branch))?;
    Ok(())
}


/// `rgit push` to a remote reached over git's smart-HTTP protocol (any https host, and
/// GitHub unless `rgit.httpTransport=api`). Pushes `src_name` (a branch, tag, `HEAD` or
/// revision; empty to delete) to `dest` on the server: asks `git-receive-pack` for its refs,
/// checks the update locally as git does (fast-forward unless `force`; existing tags only
/// with `force`), sends the ref update with a pack of the objects the server lacks, and moves
/// `refs/remotes/<remote>/<branch>` only once the server reports the ref updated. A refused
/// update leaves the local repository unchanged.
// Used by `push`.
pub fn push_over_smart_http(url: &str, remote_name: &str, remote_display: &str, src_name: &str, dest: &str, force: bool) -> Result<()> {
    use crate::remote::smart_http::receive_pack::{discover_receive_pack, send_pack, ZERO_OID};
    use crate::remote::smart_http::types::RefUpdate;
    use std::io::IsTerminal;

    let client = crate::remote::http::HttpClient::for_remote(remote_name, url)?;
    let discovery = discover_receive_pack(&client, url)?;
    let shown = crate::remote::http::redact_url(remote_display);
    let remote_value = |name: &str| discovery.refs.iter().find(|r| r.name == name).and_then(|r| r.oid.clone());
    let offered = |name: &str| discovery.capabilities.iter().any(|c| c == name);

    let (new, src_is_tag) = if src_name.is_empty() {
        (None, false)
    } else {
        let (value, is_tag) = local_push_source(src_name)?;
        (Some(value), is_tag)
    };
    let dest_ref = full_destination_ref(dest, src_is_tag, |name| remote_value(name).is_some());
    let old = remote_value(&dest_ref);
    let failed = format!("error: failed to push some refs to '{}'", shown);
    let label = if src_name.is_empty() { dest.to_string() } else { format!("{} -> {}", src_name, dest) };

    let summary = match (&old, &new) {
        (None, None) => anyhow::bail!("error: unable to delete '{}': remote ref does not exist\n{}", dest, failed),
        (Some(old), Some(new)) if old == new => {
            println!("Everything up-to-date");
            return Ok(());
        }
        (Some(_), None) => {
            if !offered("delete-refs") {
                anyhow::bail!("error: the server at '{}' does not allow deleting refs\n{}", shown, failed);
            }
            format!(" - [deleted]         {}", dest)
        }
        (None, Some(_)) if dest_ref.starts_with("refs/tags/") => format!(" * [new tag]         {}", label),
        (None, Some(_)) if dest_ref.starts_with("refs/heads/") => format!(" * [new branch]      {}", label),
        (None, Some(_)) => format!(" * [new reference]   {}", label),
        (Some(old), Some(new)) => {
            let rejection = if dest_ref.starts_with("refs/tags/") {
                Some(("already exists", "hint: Updates were rejected because the tag already exists in the remote."))
            } else if !crate::remote::fetch::engine::local_has_object(old) {
                Some((
                    "fetch first",
                    "hint: Updates were rejected because the remote contains work that you do not\n\
                     hint: have locally. Integrate the remote changes (e.g. 'rgit pull')\n\
                     hint: before pushing again.",
                ))
            } else if !crate::core::commit::is_reachable(new, old)? {
                Some((
                    "non-fast-forward",
                    "hint: Updates were rejected because the tip of your current branch is behind\n\
                     hint: its remote counterpart. Integrate the remote changes (e.g.\n\
                     hint: 'rgit merge') before pushing again.",
                ))
            } else {
                None
            };
            match rejection {
                None => format!("   {}..{}  {}", crate::core::commit::short_hash(old), crate::core::commit::short_hash(new), label),
                Some(_) if force => format!(
                    " + {}...{} {} (forced update)",
                    crate::core::commit::short_hash(old),
                    crate::core::commit::short_hash(new),
                    label
                ),
                Some((reason, hint)) => {
                    eprintln!("To {}", shown);
                    if crate::core::color::stderr_enabled("push") {
                        let red = crate::core::color::slot("push", "error", crate::core::color::RED);
                        eprintln!(" {}! [rejected]       {} {} ({})", red, crate::core::color::RESET, label, reason);
                    } else {
                        eprintln!(" ! [rejected]        {} ({})", label, reason);
                    }
                    anyhow::bail!("{}\n{}", failed, hint);
                }
            }
        }
    };

    // The objects to send: what `new` reaches minus what the server's refs already reach.
    let pack = match &new {
        Some(new) => {
            let server_tips: Vec<String> = discovery
                .refs
                .iter()
                .flat_map(|r| [r.oid.clone(), r.peeled.clone()])
                .flatten()
                .filter(|oid| crate::remote::fetch::engine::local_has_object(oid))
                .collect();
            let objects = objects_to_push(new, &server_tips);
            let window = if offered("ofs-delta") { 10 } else { 0 };
            let built = crate::core::pack::build_pack(&objects, &crate::core::types::PackBuildOptions { window, depth: 50 })?;
            println!("Enumerating objects: {}, done.", objects.len());
            if !objects.is_empty() {
                println!("Writing objects: 100% ({}/{}), done.", objects.len(), objects.len());
            }
            println!("Total {} (delta {}), reused 0 (delta 0)", objects.len(), built.deltas);
            built.bytes
        }
        None => Vec::new(),
    };

    let update = RefUpdate {
        name: dest_ref.clone(),
        old: old.clone().unwrap_or_else(|| ZERO_OID.to_string()),
        new: new.clone().unwrap_or_else(|| ZERO_OID.to_string()),
    };
    let report = send_pack(&client, &discovery, url, std::slice::from_ref(&update), &pack, std::io::stderr().is_terminal())?;
    let refused = match (&report.unpack_error, report.refs.iter().find(|(name, _)| name == &dest_ref)) {
        (Some(reason), _) => {
            eprintln!("error: remote unpack failed: {}", reason);
            Some("unpacker error".to_string())
        }
        (None, Some((_, Some(reason)))) => Some(reason.clone()),
        (None, Some((_, None))) => None,
        (None, None) => Some("the server did not report on this ref".to_string()),
    };
    if let Some(reason) = refused {
        eprintln!("To {}", shown);
        eprintln!(" ! [remote rejected] {} ({})", label, reason);
        anyhow::bail!("{}", failed);
    }

    if let Some(branch) = dest_ref.strip_prefix("refs/heads/")
        && !remote_name.contains('/')
        && !remote_name.contains('\\')
    {
        let _ = match &new {
            Some(new) => write_remote_tracking_ref(remote_name, branch, new),
            None => delete_remote_tracking_ref(remote_name, branch),
        };
    }
    println!("To {}", shown);
    println!("{}", summary);
    Ok(())
}

/// What a push of `src` sends and whether it is a tag: the value of the ref it names (an
/// annotated tag stays the tag object, as git pushes it), tried as git does: a full
/// `refs/...` name, then `refs/<src>`, `refs/tags/<src>`, `refs/heads/<src>`,
/// `refs/remotes/<src>`; otherwise `HEAD` or any revision, resolved to its commit.
// Used by `push_over_smart_http`.
pub fn local_push_source(src: &str) -> Result<(String, bool)> {
    if src == "HEAD" {
        let head = crate::core::refs::resolve_head_commit()?.ok_or_else(|| anyhow::anyhow!("error: HEAD has no commits yet"))?;
        return Ok((head, false));
    }
    let refs = crate::core::refs::list_all_refs();
    let candidates = [
        src.to_string(),
        format!("refs/{}", src),
        format!("refs/tags/{}", src),
        format!("refs/heads/{}", src),
        format!("refs/remotes/{}", src),
    ];
    for candidate in candidates.iter().filter(|c| c.starts_with("refs/")) {
        if let Some(value) = refs.get(candidate) {
            return Ok((value.clone(), candidate.starts_with("refs/tags/")));
        }
    }
    let commit = crate::core::commit::resolve_commit_from_source(src).map_err(|_| anyhow::anyhow!("error: src refspec '{}' does not match any", src))?;
    Ok((commit, false))
}

/// The ref a push updates on the server for the destination `dest`: a full `refs/...` name as
/// is; otherwise the branch or tag of that name the server already has (`remote_has`), or a
/// new tag when pushing a tag, else a new branch.
// Used by `push_over_smart_http`.
pub fn full_destination_ref(dest: &str, src_is_tag: bool, remote_has: impl Fn(&str) -> bool) -> String {
    if dest.starts_with("refs/") {
        return dest.to_string();
    }
    let branch = format!("refs/heads/{}", dest);
    let tag = format!("refs/tags/{}", dest);
    if remote_has(&branch) {
        branch
    } else if remote_has(&tag) || src_is_tag {
        tag
    } else {
        branch
    }
}

/// The objects to send so the server has everything `tip` reaches, given the tips it
/// already has (`server_tips`, all present locally): commits and tags the server lacks and
/// the trees and blobs they reach, minus everything in the trees of the commits where the
/// new history meets the server's (git's "edge" objects), which the server has.
// Used by `push_over_smart_http`.
pub fn objects_to_push(tip: &str, server_tips: &[String]) -> Vec<String> {
    use crate::core::object_store::read_object;
    let links = |hash: &str| -> Vec<String> {
        let Ok((kind, content)) = read_object(hash) else { return Vec::new() };
        if kind != "commit" && kind != "tag" {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&content);
        text.lines()
            .take_while(|l| !l.is_empty())
            .filter_map(|l| l.strip_prefix("parent ").or_else(|| l.strip_prefix("object ")))
            .map(|s| s.trim().to_string())
            .collect()
    };

    let mut on_server: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = server_tips.to_vec();
    while let Some(hash) = stack.pop() {
        if on_server.insert(hash.clone()) {
            stack.extend(links(&hash));
        }
    }

    let mut edges: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack = vec![tip.to_string()];
    while let Some(hash) = stack.pop() {
        if on_server.contains(&hash) {
            edges.push(hash);
        } else if seen.insert(hash.clone()) {
            stack.extend(links(&hash));
        }
    }

    let mut excluded = on_server;
    for edge in edges {
        if let Ok(tree) = crate::core::object_store::tree_hash_of_commit(&edge) {
            excluded.extend(crate::core::pack::reachable_objects(&[tree], &HashSet::new()));
        }
    }
    crate::core::pack::reachable_objects(&[tip.to_string()], &excluded)
}
