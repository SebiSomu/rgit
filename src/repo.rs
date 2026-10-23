use anyhow::{Context, Result};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Where the repository behind the current directory keeps its data when the current
/// directory is a linked worktree (its `.git` is a file, not a directory).
#[derive(Clone)]
struct LinkedDirs {
    /// `.git/worktrees/<id>` of the main repository: HEAD, index and in-progress state.
    git_dir: PathBuf,
    /// The main repository's `.git`: objects, refs, config, logs.
    common_dir: PathBuf,
}

/// The last linked worktree resolved, keyed by the working directory it was resolved from, so
/// object-heavy commands don't re-read `.git` and `commondir` for every path.
static LINKED_CACHE: Mutex<Option<(PathBuf, LinkedDirs)>> = Mutex::new(None);

/// Top-level entries of `.git` that every worktree of a repository shares; anything else
/// (HEAD, index, MERGE_HEAD, REBASE_*, BISECT_*, rebase-apply, FETCH_HEAD, ...) belongs to a
/// single worktree. Mirrors git's own common-dir table.
const SHARED_ENTRIES: &[&str] = &[
    "objects", "refs", "logs", "config", "worktrees", "hooks", "info", "packed-refs", "shallow",
    "description", "remotes", "branches", "STASH_LIST",
];

/// Refs (and their reflogs) that stay per-worktree even though they live under `refs/`.
const PER_WORKTREE_REF_PREFIXES: &[&str] = &["refs/bisect", "refs/worktree", "refs/rewritten"];

/// Turns a repository path written the way it looks in a main worktree (`.git/HEAD`,
/// `.git/refs/heads/main`, `.git/objects/ab/cd...`) into where that file really lives. In a
/// main worktree, or outside any repository, the path is returned unchanged. In a linked
/// worktree, shared data resolves into the main repository's `.git` and per-worktree data into
/// `.git/worktrees/<id>`.
// Used by every command and helper that reads or writes inside `.git`.
pub fn path(repo_path: impl AsRef<str>) -> PathBuf {
    let repo_path = repo_path.as_ref();
    let linked = match linked_dirs() {
        Some(dirs) => dirs,
        None => return PathBuf::from(repo_path),
    };

    let rest = match repo_path.strip_prefix(".git") {
        Some("") => return linked.git_dir,
        Some(rest) if rest.starts_with('/') => rest.trim_start_matches('/'),
        _ => return PathBuf::from(repo_path),
    };

    if is_shared(rest) {
        linked.common_dir.join(rest)
    } else {
        linked.git_dir.join(rest)
    }
}

/// Says whether a path relative to `.git` is shared by all worktrees.
// Used by `path`.
fn is_shared(rest: &str) -> bool {
    let ref_part = rest.strip_prefix("logs/").unwrap_or(rest);
    if rest == "logs/HEAD" {
        return false;
    }
    if PER_WORKTREE_REF_PREFIXES
        .iter()
        .any(|p| ref_part == *p || ref_part.starts_with(&format!("{}/", p)))
    {
        return false;
    }
    let first = rest.split('/').next().unwrap_or("");
    SHARED_ENTRIES.contains(&first)
}

/// Says whether the current directory is the top of a worktree: it has a `.git` directory, or
/// a `.git` file that points at a valid worktree administrative directory.
// Used by `config`, `count-objects`, `rev-parse` and the `worktree` command.
pub fn exists() -> bool {
    Path::new(".git").is_dir() || linked_dirs().is_some()
}

/// Says whether the current directory is a linked worktree rather than the main one.
// Used by the `worktree` command and `rev-parse --git-dir`.
pub fn is_linked_worktree() -> bool {
    !Path::new(".git").is_dir() && linked_dirs().is_some()
}

/// The repository's shared `.git` directory: `.git` in a main worktree, the main repository's
/// absolute `.git` path in a linked one.
// Used by the `worktree` command and its helpers.
pub fn common_dir() -> PathBuf {
    match linked_dirs() {
        Some(dirs) => dirs.common_dir,
        None => PathBuf::from(".git"),
    }
}

/// The current worktree's own `.git` directory: `.git` in a main worktree,
/// `.git/worktrees/<id>` (absolute) in a linked one.
// Used by `rev-parse --git-dir` and the `worktree` command.
pub fn git_dir() -> PathBuf {
    match linked_dirs() {
        Some(dirs) => dirs.git_dir,
        None => PathBuf::from(".git"),
    }
}

/// Resolves the current directory's `.git` file, or returns `None` when `.git` is a directory
/// or missing (or the file is not a usable `gitdir:` pointer).
// Used by `path`, `exists`, `common_dir` and `git_dir`.
fn linked_dirs() -> Option<LinkedDirs> {
    let dot_git = Path::new(".git");
    if !dot_git.is_file() {
        return None;
    }
    let cwd = std::env::current_dir().ok()?;

    if let Ok(cache) = LINKED_CACHE.lock() {
        if let Some((cached_cwd, dirs)) = cache.as_ref() {
            if *cached_cwd == cwd {
                return Some(dirs.clone());
            }
        }
    }

    let dirs = read_gitdir_file(&cwd, dot_git).ok()?;
    if let Ok(mut cache) = LINKED_CACHE.lock() {
        *cache = Some((cwd, dirs.clone()));
    }
    Some(dirs)
}

/// Parses a `gitdir: <path>` file and the `commondir` file next to the directory it names.
// Used by `linked_dirs`.
fn read_gitdir_file(cwd: &Path, dot_git: &Path) -> Result<LinkedDirs> {
    let content = fs::read_to_string(dot_git).context("Failed to read .git file")?;
    let target = content
        .trim()
        .strip_prefix("gitdir:")
        .map(str::trim)
        .context("invalid .git file")?;

    let git_dir = normalize(&cwd.join(target));
    if !git_dir.is_dir() {
        anyhow::bail!("fatal: not a git repository: {}", git_dir.display());
    }

    let common_dir = match fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => normalize(&git_dir.join(text.trim())),
        Err(_) => git_dir.clone(),
    };

    Ok(LinkedDirs { git_dir, common_dir })
}

/// Makes a path absolute and folds away `.` and `..` components without touching the file
/// system (unlike `canonicalize`, which yields `\\?\` paths on Windows).
// Used by `read_gitdir_file` and the `worktree` helpers.
pub fn normalize(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Writes a path the way git stores it in `gitdir` files: forward slashes on every platform.
// Used by the `worktree` helpers.
pub fn to_git_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::is_shared;

    #[test]
    fn routes_shared_and_per_worktree_paths() {
        assert!(is_shared("objects/ab/cdef"));
        assert!(is_shared("refs/heads/main"));
        assert!(is_shared("logs/refs/heads/main"));
        assert!(is_shared("config"));
        assert!(is_shared("STASH_LIST"));
        assert!(is_shared("logs"));
        assert!(!is_shared("HEAD"));
        assert!(!is_shared("index"));
        assert!(!is_shared("logs/HEAD"));
        assert!(!is_shared("MERGE_HEAD"));
        assert!(!is_shared("rebase-apply/next"));
        assert!(!is_shared("refs/bisect/bad"));
        assert!(!is_shared("logs/refs/bisect/bad"));
        assert!(!is_shared("refs/worktree"));
    }
}
