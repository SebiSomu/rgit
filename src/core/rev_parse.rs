use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::core::object_store::{read_object, tree_hash_of_commit};
use crate::core::refs::{
    list_branches, list_remote_tracking_branches, list_tags, read_ref, resolve_head,
    resolve_head_commit,
};

/// Walks up from the current working directory to find the root of the
/// working tree (the directory that contains `.git/`).
// Used by `run_rev_parse` for `--show-toplevel` and `--is-inside-work-tree`.
pub fn find_work_tree_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir().context("Cannot determine current directory")?;
    loop {
        if is_work_tree_top(&dir) {
            return Ok(dir);
        }
        if !dir.pop() {
            anyhow::bail!("fatal: not a git repository (or any of the parent directories): .git");
        }
    }
}

/// Says whether `dir` is the top of a worktree: it holds a `.git` directory (main worktree) or
/// a `.git` file pointing elsewhere (linked worktree).
// Used by `find_work_tree_root` and `is_inside_work_tree`.
fn is_work_tree_top(dir: &Path) -> bool {
    let dot_git = dir.join(".git");
    dot_git.is_dir()
        || std::fs::read_to_string(&dot_git).is_ok_and(|s| s.starts_with("gitdir:"))
}

/// Returns `true` if the current working directory is inside a git work-tree.
// Used by `run_rev_parse` for `--is-inside-work-tree`.
pub fn is_inside_work_tree() -> bool {
    let mut dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => return false,
    };
    loop {
        if is_work_tree_top(&dir) {
            return true;
        }
        if !dir.pop() {
            return false;
        }
    }
}

/// Peels a tag object until the underlying object of `target_type` is reached.
/// Passing `target_type = ""` peels all the way to the first non-tag object.
/// Also dereferences a commit object to its tree if `target_type = "tree"`.
// Used by `resolve_rev` for the `^{type}` and `^{}` suffix syntax.
pub fn peel_to_type(mut hash: String, target_type: &str) -> Result<String> {
    loop {
        let (obj_type, content) = read_object(&hash)?;
        if target_type.is_empty() {
            if obj_type != "tag" {
                return Ok(hash);
            }
        } else if obj_type == target_type {
            return Ok(hash);
        } else if obj_type == "commit" && target_type == "tree" {
            return tree_hash_of_commit(&hash);
        }

        if obj_type != "tag" {
            anyhow::bail!(
                "error: {} is a {}, not a {}",
                hash,
                obj_type,
                target_type
            );
        }

        let text = String::from_utf8_lossy(&content);
        let next = text
            .lines()
            .find_map(|line| line.strip_prefix("object "))
            .with_context(|| format!("Malformed tag object {}: missing 'object' line", hash))?
            .to_string();
        hash = next;
    }
}

/// Resolves the Nth parent of `hash` following caret-parent syntax (`^N`).
/// `n = 1` means the first parent (equivalent to `^`).
// Used by `resolve_rev` for the `^N` suffix.
pub fn resolve_nth_parent(hash: &str, n: usize) -> Result<String> {
    let (obj_type, content) = read_object(hash)?;
    if obj_type != "commit" {
        anyhow::bail!("error: {} is not a commit object", hash);
    }
    let text = String::from_utf8_lossy(&content);
    let parents: Vec<String> = text
        .lines()
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.strip_prefix("parent "))
        .map(|s| s.to_string())
        .collect();

    if n == 0 {
        return Ok(hash.to_string());
    }
    if parents.is_empty() {
        anyhow::bail!("error: commit {} has no parents", hash);
    }
    if n > parents.len() {
        anyhow::bail!(
            "error: commit {} has only {} parent(s), but ^{} was requested",
            hash,
            parents.len(),
            n
        );
    }
    Ok(parents[n - 1].clone())
}

/// Resolves a `~N` ancestor chain from `hash`.
// Used by `resolve_rev` for `~N` suffix.
pub fn resolve_tilde_ancestor(mut hash: String, n: usize) -> Result<String> {
    for _ in 0..n {
        let (obj_type, content) = read_object(&hash)?;
        if obj_type != "commit" {
            anyhow::bail!("error: {} is not a commit object", hash);
        }
        let text = String::from_utf8_lossy(&content);
        let parent = text
            .lines()
            .take_while(|l| !l.is_empty())
            .find_map(|l| l.strip_prefix("parent "))
            .with_context(|| format!("error: commit {} has no parent", hash))?
            .to_string();
        hash = parent;
    }
    Ok(hash)
}

/// Finds the object hash of the entry called `name` in a raw tree object.
fn tree_entry_hash(tree: &[u8], name: &str) -> Option<String> {
    let mut pos = 0;
    while pos < tree.len() {
        let space = tree[pos..].iter().position(|&b| b == b' ')? + pos;
        let nul = tree[space..].iter().position(|&b| b == 0)? + space;
        let hash = tree.get(nul + 1..nul + 21)?;
        if &tree[space + 1..nul] == name.as_bytes() {
            return Some(hex::encode(hash));
        }
        pos = nul + 21;
    }
    None
}

/// Full revision resolution engine.
///
/// Handles:
/// - `HEAD` (attached or detached)
/// - `<branch>`, `refs/heads/<branch>`, `refs/tags/<tag>`, `refs/remotes/<remote>/<branch>`
/// - `<raw-hash>` (verified to exist in the object store)
/// - Suffix `~N` — Nth ancestor via first-parent chain
/// - Suffix `^N` — Nth parent of the same generation
/// - Suffix `^` — shorthand for `^1`
/// - Suffix `^{type}` — peel to object of given type (`commit`, `tree`, `blob`, `tag`)
/// - Suffix `^{}` — peel tag to the underlying non-tag object
/// - `<rev>:<path>` — the blob or tree at `<path>` in `<rev>`'s tree (`<rev>:` is the tree itself)
// Used by the `rev_parse` command.
pub fn resolve_rev(rev: &str) -> Result<String> {
    if let Some((base, path)) = rev.split_once(':').filter(|(base, _)| !base.is_empty()) {
        let mut hash = peel_to_type(resolve_rev(base)?, "tree")?;
        for component in path.split('/').filter(|c| !c.is_empty()) {
            let (_, content) = read_object(&hash)?;
            hash = tree_entry_hash(&content, component)
                .ok_or_else(|| anyhow::anyhow!("fatal: path '{}' does not exist in '{}'", path, base))?;
        }
        return Ok(hash);
    }
    let mut remaining = rev.to_string();
    let mut suffixes: Vec<String> = Vec::new();

    // Strip suffixes from the right, always taking whichever one actually ends the string, so
    // mixed chains such as `topic~2^0` or `v1^{commit}~1^2` come apart correctly.
    loop {
        if remaining.ends_with('}') {
            if let Some(pos) = remaining.rfind("^{") {
                suffixes.push(remaining[pos..].to_string());
                remaining.truncate(pos);
                continue;
            }
        }
        let digits = remaining.chars().rev().take_while(|c| c.is_ascii_digit()).count();
        let marker_end = remaining.len() - digits;
        let marker = remaining[..marker_end].chars().last();
        if matches!(marker, Some('^') | Some('~')) && marker_end > 1 {
            let default = 1;
            let n: usize = if digits == 0 { default } else { remaining[marker_end..].parse().unwrap_or(default) };
            suffixes.push(format!("{}{}", marker.unwrap_or('^'), n));
            remaining.truncate(marker_end - 1);
            continue;
        }
        break;
    }

    let mut hash = resolve_base_name(&remaining)?;
    for suffix in suffixes.into_iter().rev() {
        if suffix.starts_with("^{") {
            let inner = &suffix[2..suffix.len() - 1];
            hash = peel_to_type(hash, inner)?;
        } else if let Some(rest) = suffix.strip_prefix('~') {
            let n: usize = rest.parse().unwrap_or(1);
            hash = resolve_tilde_ancestor(peel_to_type(hash, "commit")?, n)?;
        } else if let Some(rest) = suffix.strip_prefix('^') {
            // `^0` is the commit itself, which also peels an annotated tag.
            let n: usize = rest.parse().unwrap_or(1);
            hash = resolve_nth_parent(&peel_to_type(hash, "commit")?, n)?;
        }
    }

    Ok(hash)
}

/// Prints git's `warning: refname '<name>' is ambiguous.` on stderr, once per name for the
/// whole run (commands often resolve the same argument several times).
// Used by `resolve_base_name`.
fn warn_ambiguous_refname(name: &str) {
    static WARNED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut warned = WARNED.lock().unwrap_or_else(|e| e.into_inner());
    if !warned.iter().any(|n| n == name) {
        eprintln!("warning: refname '{}' is ambiguous.", name);
        warned.push(name.to_string());
    }
}

/// Reads a ref, following symbolic refs (`ref: refs/...`, e.g. `refs/remotes/origin/HEAD`) to
/// the hash they end at. Returns `None` when the ref, or the ref it points to, does not exist.
// Used by `resolve_base_name`.
fn read_ref_following_symref(ref_name: &str) -> Result<Option<String>> {
    let mut current = ref_name.to_string();
    for _ in 0..5 {
        // A name such as `feat` can be a directory of refs (`refs/heads/feat/a`), not a ref.
        if crate::core::repo::path(format!(".git/{}", current)).is_dir() {
            return Ok(None);
        }
        match read_ref(&current)? {
            Some(value) => match value.strip_prefix("ref: ") {
                Some(target) => current = target.trim().to_string(),
                None => return Ok(Some(value)),
            },
            None => return Ok(None),
        }
    }
    Ok(None)
}

/// Resolves a bare name (no suffixes) into a commit/object hash.
// Used by `resolve_rev`.
fn resolve_base_name(name: &str) -> Result<String> {
    // Reflog spec (e.g. HEAD@{1}, main@{0}, @{1})
    if let Some(hash) = crate::core::reflog::resolve_reflog_revision(name)? {
        return Ok(hash);
    }

    // HEAD
    if name.eq_ignore_ascii_case("head") || name == "@" {
        let hash = resolve_head_commit()?.ok_or_else(|| {
            anyhow::anyhow!("fatal: ambiguous argument 'HEAD': unknown revision or path not in the working tree")
        })?;
        return Ok(hash);
    }

    // A full object name always wins over a ref that happens to be called the same.
    if name.len() == 40 && name.chars().all(|c| c.is_ascii_hexdigit()) && read_object(name).is_ok() {
        return Ok(name.to_string());
    }

    // Ref lookup in git's order: the name itself (`refs/...` or a pseudo-ref like `ORIG_HEAD`),
    // `refs/<name>` (so `heads/x`, `tags/x`, `remotes/x` work), then tag, branch, remote-tracking
    // branch and a remote's default branch. The first match wins; more than one match is ambiguous.
    let pseudo_ref = !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_');
    let mut candidates: Vec<String> = Vec::new();
    if name.starts_with("refs/") || pseudo_ref {
        candidates.push(name.to_string());
    }
    for prefix in ["refs/", "refs/tags/", "refs/heads/", "refs/remotes/"] {
        candidates.push(format!("{}{}", prefix, name));
    }
    candidates.push(format!("refs/remotes/{}/HEAD", name));

    let mut found: Option<String> = None;
    let mut matches = 0;
    for candidate in &candidates {
        if let Some(hash) = read_ref_following_symref(candidate)? {
            matches += 1;
            found.get_or_insert(hash);
        }
    }
    if let Some(hash) = found {
        if matches > 1 {
            warn_ambiguous_refname(name);
        }
        return Ok(hash);
    }

    // Bare hash or prefix — verify it exists
    if name.len() >= 4 && name.chars().all(|c| c.is_ascii_hexdigit()) {
        if name.len() == 40 {
            if read_object(name).is_ok() {
                return Ok(name.to_string());
            }
        } else {
            // Candidates: loose objects in `objects/<xx>/` and the names in the pack indexes.
            let prefix = name.to_ascii_lowercase();
            let mut matches = crate::core::pack::packed_object_names(&prefix);
            let prefix_dir = crate::core::repo::path(format!(".git/objects/{}", &prefix[..2]));
            if let Ok(entries) = std::fs::read_dir(prefix_dir) {
                let rest = &prefix[2..];
                for entry in entries.flatten() {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if file_name.starts_with(rest) {
                        matches.push(format!("{}{}", &prefix[..2], file_name));
                    }
                }
            }
            matches.sort();
            matches.dedup();
            if matches.len() == 1 {
                return Ok(matches.remove(0));
            } else if matches.len() > 1 {
                anyhow::bail!("short SHA1 {} is ambiguous", name);
            }
        }
    }

    anyhow::bail!(
        "fatal: ambiguous argument '{}': unknown revision or path not in the working tree",
        name
    )
}

/// Converts a hash to its symbolic name (`HEAD`, branch name, tag, remote-tracking branch).
/// Returns `HEAD` when HEAD is detached and pointing at `hash`, or the hash itself when
/// no name is found.
// Used by the `rev_parse` command for `--abbrev-ref`.
pub fn abbrev_ref_for(hash: &str) -> Result<String> {
    match resolve_head()? {
        crate::core::refs::HeadState::Branch(branch) => {
            if let Some(tip) = read_ref(&format!("refs/heads/{}", branch))? {
                if tip == hash {
                    return Ok(branch);
                }
            }
        }
        crate::core::refs::HeadState::Detached(h) if h == hash => {
            return Ok("HEAD".to_string());
        }
        _ => {}
    }

    for b in list_branches()? {
        if let Some(tip) = read_ref(&format!("refs/heads/{}", b))? {
            if tip == hash {
                return Ok(b);
            }
        }
    }
    for t in list_tags()? {
        if let Some(tip) = read_ref(&format!("refs/tags/{}", t))? {
            if tip == hash {
                return Ok(t);
            }
        }
    }
    let remotes_dir = &crate::core::repo::path(".git/refs/remotes");
    if remotes_dir.is_dir() {
        for entry in std::fs::read_dir(remotes_dir)
            .with_context(|| "Failed to read .git/refs/remotes")?
            .flatten()
        {
            let remote = entry.file_name().to_string_lossy().to_string();
            for b in list_remote_tracking_branches(&remote)? {
                let ref_name = format!("refs/remotes/{}/{}", remote, b);
                if let Some(tip) = read_ref(&ref_name)? {
                    if tip == hash {
                        return Ok(format!("{}/{}", remote, b));
                    }
                }
            }
        }
    }

    Ok(hash.to_string())
}

/// Like `abbrev_ref_for` but returns the full `refs/heads/…` path.
// Used by the `rev_parse` command for `--symbolic-full-name`.
pub fn symbolic_full_name_for(hash: &str) -> Result<String> {
    match resolve_head()? {
        crate::core::refs::HeadState::Branch(branch) => {
            if let Some(tip) = read_ref(&format!("refs/heads/{}", branch))? {
                if tip == hash {
                    return Ok(format!("refs/heads/{}", branch));
                }
            }
        }
        crate::core::refs::HeadState::Detached(h) if h == hash => {
            return Ok("HEAD".to_string());
        }
        _ => {}
    }

    for b in list_branches()? {
        if let Some(tip) = read_ref(&format!("refs/heads/{}", b))? {
            if tip == hash {
                return Ok(format!("refs/heads/{}", b));
            }
        }
    }
    for t in list_tags()? {
        if let Some(tip) = read_ref(&format!("refs/tags/{}", t))? {
            if tip == hash {
                return Ok(format!("refs/tags/{}", t));
            }
        }
    }
    let remotes_dir = &crate::core::repo::path(".git/refs/remotes");
    if remotes_dir.is_dir() {
        for entry in std::fs::read_dir(remotes_dir)
            .with_context(|| "Failed to read .git/refs/remotes")?
            .flatten()
        {
            let remote = entry.file_name().to_string_lossy().to_string();
            for b in list_remote_tracking_branches(&remote)? {
                let ref_name = format!("refs/remotes/{}/{}", remote, b);
                if let Some(tip) = read_ref(&ref_name)? {
                    if tip == hash {
                        return Ok(ref_name);
                    }
                }
            }
        }
    }

    Ok(hash.to_string())
}

/// Special-case: `--abbrev-ref HEAD` should return the branch name even without
/// resolving to a hash first, so HEAD itself is handled symbolically.
// Used by `run_rev_parse` for the `--abbrev-ref` + `HEAD` combination.
pub fn abbrev_ref_symbolic(rev: &str) -> Result<String> {
    if rev.eq_ignore_ascii_case("head") {
        return match resolve_head()? {
            crate::core::refs::HeadState::Branch(b) => Ok(b),
            crate::core::refs::HeadState::Detached(_) => Ok("HEAD".to_string()),
        };
    }
    let hash = resolve_rev(rev)?;
    abbrev_ref_for(&hash)
}

/// Special-case: `--symbolic-full-name HEAD` returns `refs/heads/<branch>`.
// Used by `run_rev_parse` for the `--symbolic-full-name` + `HEAD` combination.
pub fn symbolic_full_name_symbolic(rev: &str) -> Result<String> {
    if rev.eq_ignore_ascii_case("head") {
        return match resolve_head()? {
            crate::core::refs::HeadState::Branch(b) => Ok(format!("refs/heads/{}", b)),
            crate::core::refs::HeadState::Detached(_) => Ok("HEAD".to_string()),
        };
    }
    let hash = resolve_rev(rev)?;
    symbolic_full_name_for(&hash)
}

#[cfg(test)]
#[path = "../../tests/unit/core/rev_parse.rs"]
mod tests;
