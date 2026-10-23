use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::helpers::objects::{read_object, tree_hash_of_commit};
use crate::refs::{
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
// Used by the `rev_parse` command.
pub fn resolve_rev(rev: &str) -> Result<String> {
    let mut remaining = rev.to_string();
    let mut suffixes: Vec<String> = Vec::new();

    loop {
        if let Some(pos) = remaining.rfind("^{") {
            if remaining.ends_with('}') {
                let suffix = remaining[pos..].to_string();
                remaining = remaining[..pos].to_string();
                suffixes.push(suffix);
                continue;
            }
        }
        if let Some(pos) = remaining.rfind('~') {
            let after = &remaining[pos + 1..];
            let n: usize = if after.is_empty() {
                1
            } else if let Ok(n) = after.parse() {
                n
            } else {
                break;
            };
            remaining = remaining[..pos].to_string();
            suffixes.push(format!("~{}", n));
            continue;
        }
        if remaining.ends_with('^') {
            remaining.pop();
            suffixes.push("^1".to_string());
            continue;
        }
        if let Some(pos) = remaining.rfind('^') {
            let after = &remaining[pos + 1..];
            if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit()) {
                let n: usize = after.parse().unwrap_or(1);
                remaining = remaining[..pos].to_string();
                suffixes.push(format!("^{}", n));
                continue;
            }
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
            hash = resolve_tilde_ancestor(hash, n)?;
        } else if let Some(rest) = suffix.strip_prefix('^') {
            let n: usize = rest.parse().unwrap_or(1);
            hash = resolve_nth_parent(&hash, n)?;
        }
    }

    Ok(hash)
}

/// Resolves a bare name (no suffixes) into a commit/object hash.
// Used by `resolve_rev`.
fn resolve_base_name(name: &str) -> Result<String> {
    // Reflog spec (e.g. HEAD@{1}, main@{0}, @{1})
    if let Some(hash) = crate::helpers::reflog::resolve_reflog_revision(name)? {
        return Ok(hash);
    }

    // HEAD
    if name.eq_ignore_ascii_case("head") {
        let hash = resolve_head_commit()?.ok_or_else(|| {
            anyhow::anyhow!("fatal: ambiguous argument 'HEAD': unknown revision or path not in the working tree")
        })?;
        return Ok(hash);
    }

    // Full ref path (refs/…)
    if name.starts_with("refs/") {
        if let Some(hash) = read_ref(name)? {
            return Ok(hash);
        }
        anyhow::bail!("fatal: ambiguous argument '{}': unknown revision or path not in the working tree", name);
    }

    // Local branch
    if let Some(hash) = read_ref(&format!("refs/heads/{}", name))? {
        return Ok(hash);
    }
    // Tag
    if let Some(hash) = read_ref(&format!("refs/tags/{}", name))? {
        return Ok(hash);
    }
    // Remote-tracking branch (e.g. `origin/main`)
    if let Some(hash) = read_ref(&format!("refs/remotes/{}", name))? {
        return Ok(hash);
    }

    // Bare hash or prefix — verify it exists
    if name.len() >= 4 && name.chars().all(|c| c.is_ascii_hexdigit()) {
        if name.len() == 40 {
            if read_object(name).is_ok() {
                return Ok(name.to_string());
            }
        } else {
            let prefix_dir = crate::repo::path(format!(".git/objects/{}", &name[..2]));
            if let Ok(entries) = std::fs::read_dir(prefix_dir) {
                let rest = &name[2..];
                let mut matches = Vec::new();
                for entry in entries.flatten() {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if file_name.starts_with(rest) {
                        matches.push(format!("{}{}", &name[..2], file_name));
                    }
                }
                if matches.len() == 1 {
                    return Ok(matches.remove(0));
                } else if matches.len() > 1 {
                    anyhow::bail!("short SHA1 {} is ambiguous", name);
                }
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
        crate::refs::HeadState::Branch(branch) => {
            if let Some(tip) = read_ref(&format!("refs/heads/{}", branch))? {
                if tip == hash {
                    return Ok(branch);
                }
            }
        }
        crate::refs::HeadState::Detached(h) if h == hash => {
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
    let remotes_dir = &crate::repo::path(".git/refs/remotes");
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
        crate::refs::HeadState::Branch(branch) => {
            if let Some(tip) = read_ref(&format!("refs/heads/{}", branch))? {
                if tip == hash {
                    return Ok(format!("refs/heads/{}", branch));
                }
            }
        }
        crate::refs::HeadState::Detached(h) if h == hash => {
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
    let remotes_dir = &crate::repo::path(".git/refs/remotes");
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
            crate::refs::HeadState::Branch(b) => Ok(b),
            crate::refs::HeadState::Detached(_) => Ok("HEAD".to_string()),
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
            crate::refs::HeadState::Branch(b) => Ok(format!("refs/heads/{}", b)),
            crate::refs::HeadState::Detached(_) => Ok("HEAD".to_string()),
        };
    }
    let hash = resolve_rev(rev)?;
    symbolic_full_name_for(&hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// resolve_rev should propagate the ambiguous-argument error when the name is garbage.
    #[test]
    fn test_resolve_rev_rejects_unknown_name() {
        let _guard = TEST_LOCK.lock().unwrap();
        let orig = std::env::current_dir().unwrap();
        let tmp = std::env::temp_dir().join(format!("rgit_rp_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let err = resolve_rev("this_does_not_exist_at_all").unwrap_err();
        assert!(
            err.to_string().contains("unknown revision"),
            "unexpected error: {}",
            err
        );

        std::env::set_current_dir(orig).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_rev_parse_peel_and_prefix() {
        let _guard = TEST_LOCK.lock().unwrap();
        let orig = std::env::current_dir().unwrap();
        let tmp = std::env::temp_dir().join(format!("rgit_rp_test_peel_{}", std::process::id()));
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        // Write a blob object
        let blob_data = b"hello test rev parse";
        let blob_hash = crate::helpers::objects::write_object("blob", blob_data).unwrap();

        // Write a tree containing the blob
        let mut tree_content = Vec::new();
        let mode_and_name = format!("100644 hello.txt\0");
        tree_content.extend_from_slice(mode_and_name.as_bytes());
        let raw_hash = hex::decode(&blob_hash).unwrap();
        tree_content.extend_from_slice(&raw_hash);
        let tree_hash = crate::helpers::objects::write_object("tree", &tree_content).unwrap();

        // Write a commit pointing to that tree
        let commit_data = format!(
            "tree {}\nauthor Test <test@example.com> 1234567890 +0000\ncommitter Test <test@example.com> 1234567890 +0000\n\ntest commit\n",
            tree_hash
        );
        let commit_hash = crate::helpers::objects::write_object("commit", commit_data.as_bytes()).unwrap();

        // Test resolving full hash
        assert_eq!(resolve_rev(&commit_hash).unwrap(), commit_hash);

        // Test resolving prefix (short hash)
        let short_prefix = &commit_hash[..7];
        assert_eq!(resolve_rev(short_prefix).unwrap(), commit_hash);

        // Test peeling commit to tree
        let peel_spec = format!("{}^{{tree}}", commit_hash);
        assert_eq!(resolve_rev(&peel_spec).unwrap(), tree_hash);

        // Test work tree detection
        assert!(is_inside_work_tree());

        std::env::set_current_dir(orig).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
