//! How `rgit scrub` rewrites history: trees without the scrubbed paths, then every commit
//! (parents first) and annotated tag on top of them, keeping everything else byte for byte.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::core::object_store::{read_object, write_object};
use crate::history::scrub::types::{RefChange, ScrubPlan};

/// The empty tree, which git never stores as a subtree.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Prefix of the refs that keep the old branches and tags after a scrub.
pub const BACKUP_PREFIX: &str = "refs/rgit/scrub-backup/";

/// Turns command-line paths into `/`-separated paths from the top of the repository: `./` and
/// trailing `/` dropped, `\` read as `/`. Refuses empty, absolute and `..` paths.
// Used by `rgit scrub`.
pub fn normalize_paths(raw: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for path in raw {
        let unified = path.replace('\\', "/");
        let parts: Vec<&str> = unified.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
        if parts.is_empty() || unified.starts_with('/') || unified.contains(':') || parts.contains(&"..") || parts.contains(&".git") {
            anyhow::bail!("fatal: '{}' is not a path inside the repository", path);
        }
        let joined = parts.join("/");
        if !out.contains(&joined) {
            out.push(joined);
        }
    }
    Ok(out)
}

/// Rewrites trees without the scrubbed paths, remembering what it already did.
// Used by `plan_scrub` (and the unit tests).
pub struct TreeScrubber {
    cache: HashMap<String, String>,
}

impl TreeScrubber {
    pub fn new() -> TreeScrubber {
        TreeScrubber { cache: HashMap::new() }
    }

    /// `tree` without `paths` (each split in components, relative to this tree): a matching
    /// file or folder is left out, a folder left empty disappears. Unchanged trees keep their
    /// name, so nothing is written for history that never had the paths.
    pub fn scrub(&mut self, tree: &str, paths: &[Vec<String>]) -> Result<String> {
        if paths.is_empty() {
            return Ok(tree.to_string());
        }
        let key = format!("{}:{}", tree, paths.iter().map(|p| p.join("/")).collect::<Vec<_>>().join("\0"));
        if let Some(done) = self.cache.get(&key) {
            return Ok(done.clone());
        }
        let (_, content) = read_object(tree)?;
        let mut out: Vec<u8> = Vec::with_capacity(content.len());
        let mut changed = false;
        let mut rest = content.as_slice();
        while let Some(space) = rest.iter().position(|&b| b == b' ') {
            let nul = space + rest[space..].iter().position(|&b| b == 0).ok_or_else(|| anyhow::anyhow!("fatal: tree {} is corrupt", tree))?;
            if rest.len() < nul + 21 {
                anyhow::bail!("fatal: tree {} is corrupt", tree);
            }
            let mode = &rest[..space];
            let name = String::from_utf8_lossy(&rest[space + 1..nul]).into_owned();
            let mut hash = rest[nul + 1..nul + 21].to_vec();
            let entry_len = nul + 21;
            let here: Vec<&Vec<String>> = paths.iter().filter(|p| p[0] == name).collect();
            if here.iter().any(|p| p.len() == 1) {
                changed = true;
                rest = &rest[entry_len..];
                continue;
            }
            if !here.is_empty() && mode == b"40000" {
                let inner: Vec<Vec<String>> = here.iter().map(|p| p[1..].to_vec()).collect();
                let old = hex::encode(&hash);
                let new = self.scrub(&old, &inner)?;
                if new != old {
                    changed = true;
                    if new == EMPTY_TREE {
                        rest = &rest[entry_len..];
                        continue;
                    }
                    hash = hex::decode(&new)?;
                }
            }
            out.extend_from_slice(mode);
            out.push(b' ');
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            out.extend_from_slice(&hash);
            rest = &rest[entry_len..];
        }
        let result = if changed { write_object("tree", &out)? } else { tree.to_string() };
        self.cache.insert(key, result.clone());
        Ok(result)
    }
}

impl Default for TreeScrubber {
    fn default() -> Self {
        Self::new()
    }
}

/// Plans (and, unless `dry_run`, writes the objects of) a scrub of `paths` from every commit
/// the local branches and tags reach. Commits are rewritten parents first, keeping their
/// headers (author, committer, encoding) and message byte for byte; signatures are dropped,
/// since they no longer match. A non-merge commit whose only change was to the scrubbed paths
/// is left out. Annotated tags are rewritten to point at the new commit. Nothing is moved yet:
/// `apply_scrub` does that.
// Used by `rgit scrub`.
pub fn plan_scrub(paths: &[String], dry_run: bool) -> Result<ScrubPlan> {
    let split: Vec<Vec<String>> = paths.iter().map(|p| p.split('/').map(str::to_string).collect()).collect();
    let refs: Vec<(String, String)> = crate::core::refs::list_all_refs()
        .into_iter()
        .filter(|(name, _)| name.starts_with("refs/heads/") || name.starts_with("refs/tags/"))
        .collect();

    // Every commit the refs reach, parents before children.
    let tips: Vec<String> = refs.iter().filter_map(|(_, oid)| peel_to_commit(oid)).collect();
    let order = parents_first(&tips)?;

    let mut scrubber = TreeScrubber::new();
    let mut plan = ScrubPlan { paths: paths.to_vec(), commits: order.len(), ..Default::default() };
    let mut new_tree_of: HashMap<String, String> = HashMap::new();
    for commit in &order {
        let (_, content) = read_object(commit)?;
        let text = String::from_utf8_lossy(&content).into_owned();
        let (header, _) = text.split_once("\n\n").unwrap_or((&text, ""));
        let tree = header.lines().find_map(|l| l.strip_prefix("tree ")).unwrap_or_default().to_string();
        let parents: Vec<String> = header.lines().filter_map(|l| l.strip_prefix("parent ")).map(str::to_string).collect();
        let new_tree = scrubber.scrub(&tree, &split)?;
        let mut new_parents: Vec<String> = Vec::new();
        for parent in &parents {
            let mapped = plan.mapping.get(parent).cloned().unwrap_or_else(|| parent.clone());
            if !new_parents.contains(&mapped) {
                new_parents.push(mapped);
            }
        }
        if new_tree != tree {
            plan.containing += 1;
        }
        new_tree_of.insert(commit.clone(), new_tree.clone());
        // Only the scrubbed files changed in it: leave it out.
        if let ([old_parent], [new_parent]) = (parents.as_slice(), new_parents.as_slice())
            && new_tree != tree
            && new_tree_of.get(new_parent).is_some_and(|t| *t == new_tree)
            && crate::core::object_store::tree_hash_of_commit(old_parent).is_ok_and(|t| t != tree)
        {
            plan.mapping.insert(commit.clone(), new_parent.clone());
            plan.dropped += 1;
            continue;
        }
        if new_tree == tree && new_parents == parents {
            continue;
        }
        let new_commit = if dry_run { format!("dry-run:{}", commit) } else { write_object("commit", &rewrite_commit(&content, &new_tree, &new_parents))? };
        new_tree_of.insert(new_commit.clone(), new_tree_of[commit].clone());
        plan.mapping.insert(commit.clone(), new_commit);
        plan.rewritten += 1;
    }

    for (name, oid) in &refs {
        if let Some(new) = rewritten_ref_target(oid, &plan.mapping, dry_run)? {
            plan.refs.push(RefChange { name: name.clone(), old: oid.clone(), new });
        }
    }
    Ok(plan)
}

/// A commit object with a new tree and parents; every other header kept as it was except
/// signatures (`gpgsig`, `gpgsig-sha256`, `mergetag`), which the rewrite invalidates.
// Used by `plan_scrub`.
pub fn rewrite_commit(content: &[u8], tree: &str, parents: &[String]) -> Vec<u8> {
    let text = String::from_utf8_lossy(content);
    let (header, message) = text.split_once("\n\n").unwrap_or((&text, ""));
    let mut out = format!("tree {}\n", tree);
    for parent in parents {
        out.push_str(&format!("parent {}\n", parent));
    }
    let mut skipping = false;
    for line in header.lines() {
        if line.starts_with(' ') {
            if !skipping {
                out.push_str(line);
                out.push('\n');
            }
            continue;
        }
        skipping = ["gpgsig ", "gpgsig-sha256 ", "mergetag "].iter().any(|p| line.starts_with(p));
        if skipping || line.starts_with("tree ") || line.starts_with("parent ") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(message);
    out.into_bytes()
}

/// Where a branch or tag at `oid` goes: the rewritten commit, or for an annotated tag a new tag
/// object naming it (same tagger and message, without a signature). `None` when unchanged.
// Used by `plan_scrub`.
fn rewritten_ref_target(oid: &str, mapping: &HashMap<String, String>, dry_run: bool) -> Result<Option<String>> {
    if let Some(new) = mapping.get(oid) {
        return Ok(Some(new.clone()));
    }
    let (kind, content) = read_object(oid)?;
    if kind != "tag" {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&content).into_owned();
    let Some(target) = text.lines().next().and_then(|l| l.strip_prefix("object ")) else { return Ok(None) };
    let Some(new_target) = rewritten_ref_target(target, mapping, dry_run)? else { return Ok(None) };
    if dry_run {
        return Ok(Some(format!("dry-run:{}", oid)));
    }
    let mut body = text.replacen(&format!("object {}", target), &format!("object {}", new_target), 1);
    if let Some(signature) = body.find("-----BEGIN PGP SIGNATURE-----") {
        body.truncate(signature);
    }
    Ok(Some(write_object("tag", body.as_bytes())?))
}

/// The commit an object names, through any annotated tags; `None` for trees and blobs.
// Used by `plan_scrub`.
fn peel_to_commit(oid: &str) -> Option<String> {
    let mut current = oid.to_string();
    for _ in 0..16 {
        let (kind, content) = read_object(&current).ok()?;
        match kind.as_str() {
            "commit" => return Some(current),
            "tag" => current = String::from_utf8_lossy(&content).lines().next()?.strip_prefix("object ")?.to_string(),
            _ => return None,
        }
    }
    None
}

/// The commits `tips` reach, each after all of its parents.
// Used by `plan_scrub`.
fn parents_first(tips: &[String]) -> Result<Vec<String>> {
    let mut order = Vec::new();
    let mut done: HashSet<String> = HashSet::new();
    let mut stack: Vec<(String, bool)> = tips.iter().rev().map(|t| (t.clone(), false)).collect();
    while let Some((commit, parents_done)) = stack.pop() {
        if done.contains(&commit) {
            continue;
        }
        if parents_done {
            done.insert(commit.clone());
            order.push(commit);
            continue;
        }
        let parents = crate::core::commit::parse_commit_object(&commit)?.parents;
        stack.push((commit, true));
        for parent in parents.into_iter().rev() {
            if !done.contains(&parent) {
                stack.push((parent, false));
            }
        }
    }
    Ok(order)
}

/// Moves the refs of a plan, saving each old value under `refs/rgit/scrub-backup/` first, and
/// takes the scrubbed paths out of the index when HEAD moved (the files stay on disk,
/// untracked).
// Used by `rgit scrub`.
pub fn apply_scrub(plan: &ScrubPlan) -> Result<()> {
    let head_before = crate::core::refs::resolve_head_commit()?;
    crate::core::reflog::set_reflog_action(&format!("scrub: remove {}", plan.paths.join(", ")));
    for change in &plan.refs {
        let backup = format!("{}{}", BACKUP_PREFIX, change.name.trim_start_matches("refs/"));
        crate::core::refs::write_ref(&backup, &change.old)?;
    }
    for change in &plan.refs {
        crate::core::refs::write_ref(&change.name, &change.new)?;
        // A stale `packed-refs` line would keep the old history alive for gc.
        crate::core::refs::remove_packed_ref(&change.name)?;
    }
    if let crate::core::refs::HeadState::Detached(_) = crate::core::refs::resolve_head()?
        && let Some(head) = &head_before
        && let Some(new) = plan.mapping.get(head)
    {
        crate::core::refs::set_head_detached(new)?;
    }
    if crate::core::refs::resolve_head_commit()? != head_before {
        let mut entries = crate::core::index::read_index().unwrap_or_default();
        entries.retain(|e| !plan.paths.iter().any(|p| e.path == *p || e.path.starts_with(&format!("{}/", p))));
        crate::core::index::write_index(&mut entries)?;
    }
    Ok(())
}

/// The refs holding a previous scrub's backup.
// Used by `rgit scrub` and `rgit scrub --drop-backup`.
pub fn backup_refs() -> Vec<String> {
    crate::core::refs::list_all_refs().into_keys().filter(|name| name.starts_with(BACKUP_PREFIX)).collect()
}

/// Deletes a previous scrub's backup refs (loose and packed) and returns how many there were.
// Used by `rgit scrub --drop-backup`.
pub fn drop_backup() -> Result<usize> {
    let refs = backup_refs();
    for name in &refs {
        let path = crate::core::repo::path(format!(".git/{}", name));
        if path.exists() {
            std::fs::remove_file(&path)?;
            crate::core::refs::prune_empty_ref_dirs(&path);
        }
        crate::core::refs::remove_packed_ref(name)?;
        crate::core::reflog::delete_reflog(name);
    }
    Ok(refs.len())
}

#[cfg(test)]
#[path = "../../../tests/unit/history/scrub.rs"]
mod tests;
