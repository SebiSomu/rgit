use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;

use crate::core::config::{get_from, read_entries_from, LOCAL_CONFIG_PATH};
use crate::core::object_store::{hash_content, write_object};
use crate::core::index::{build_entry, read_index, write_index};
use crate::workspace::submodule::types::SubmoduleInfo;

/// Usage text of `submodule`.
pub const SUBMODULE_USAGE: &str = "usage: rgit submodule [--quiet] [--cached]\n   or: rgit submodule add [-b <branch>] [--name <name>] [--] <repository> [<path>]\n   or: rgit submodule status [--cached] [--recursive] [--] [<path>...]\n   or: rgit submodule init [--] [<path>...]\n   or: rgit submodule deinit [-f|--force] (--all| [--] <path>...)\n   or: rgit submodule update [--init] [--recursive] [--] [<path>...]\n   or: rgit submodule set-branch (--default|--branch <branch>) [--] <path>\n   or: rgit submodule set-url [--] <path> <newurl>\n   or: rgit submodule foreach [--recursive] <command>\n   or: rgit submodule sync [--] [<path>...]\n";

/// The git directory of the repository checked out at `path`: `<path>/.git` when it is a
/// directory, or the directory a `gitdir: <dir>` file there points to.
// Used by `submodule_head` and the `submodule` command.
pub fn submodule_git_dir(path: &str) -> Option<PathBuf> {
    let dot_git = Path::new(path).join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = fs::read_to_string(&dot_git).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    let dir = if Path::new(target).is_absolute() { PathBuf::from(target) } else { Path::new(path).join(target) };
    dir.is_dir().then_some(dir)
}

/// The commit checked out in the repository at `path` (its `HEAD`, through a branch if it is
/// on one). `None` when there is no repository there or it has no commit.
// Used by `status`, `add` and the `submodule` command.
pub fn submodule_head(path: &str) -> Option<String> {
    let git_dir = submodule_git_dir(path)?;
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let is_hash = |text: &str| text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit());
    let Some(reference) = head.strip_prefix("ref: ") else { return is_hash(head).then(|| head.to_string()) };
    if let Ok(value) = fs::read_to_string(git_dir.join(reference.trim())) {
        return is_hash(value.trim()).then(|| value.trim().to_string());
    }
    let packed = fs::read_to_string(git_dir.join("packed-refs")).ok()?;
    packed.lines().filter_map(|l| l.split_once(' ')).find(|(_, name)| *name == reference.trim()).map(|(hash, _)| hash.to_string())
}

/// The submodules listed in `.gitmodules`, each with the commit staged for it (if its path
/// is a gitlink in the index), sorted by path.
// Used by the `submodule` command.
pub fn read_gitmodules() -> Result<Vec<SubmoduleInfo>> {
    let mut by_name: BTreeMap<String, SubmoduleInfo> = BTreeMap::new();
    let file = Path::new(".gitmodules");
    if file.exists() {
        for (key, value) in read_entries_from(file)? {
            let Some(rest) = key.strip_prefix("submodule.") else { continue };
            let Some((name, field)) = rest.rsplit_once('.') else { continue };
            let info = by_name.entry(name.to_string()).or_insert_with(|| SubmoduleInfo { name: name.to_string(), ..Default::default() });
            match field {
                "path" => info.path = value,
                "url" => info.url = value,
                "branch" => info.branch = Some(value),
                _ => {}
            }
        }
    }
    let index = read_index().unwrap_or_default();
    let mut modules: Vec<SubmoduleInfo> = by_name.into_values().filter(|m| !m.path.is_empty()).collect();
    for module in &mut modules {
        module.staged = index.iter().find(|e| e.path == module.path && e.mode == 0o160000).map(|e| hex::encode(e.hash));
    }
    modules.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(modules)
}

/// The URL a submodule is registered with in `.git/config` (set by `submodule init`), if any.
// Used by the `submodule` command.
pub fn registered_url(name: &str) -> Option<String> {
    get_from(&crate::core::repo::path(LOCAL_CONFIG_PATH), &format!("submodule.{}.url", name)).ok().flatten()
}

/// Turns a submodule URL into something that can be cloned from here: a relative path
/// (`./x`, `../x`) is taken relative to the superproject's `origin` URL when that is a
/// local path, else relative to the superproject itself.
// Used by `submodule add` / `update` / `sync`.
pub fn resolve_submodule_url(url: &str) -> String {
    if !(url.starts_with("./") || url.starts_with("../")) {
        return url.to_string();
    }
    let origin = get_from(&crate::core::repo::path(LOCAL_CONFIG_PATH), "remote.origin.url").ok().flatten();
    let base = match origin {
        Some(origin) if !origin.contains("://") && !origin.contains('@') => PathBuf::from(origin),
        _ => std::env::current_dir().unwrap_or_default(),
    };
    crate::core::repo::normalize(&base.join(url)).to_string_lossy().replace('\\', "/")
}

/// Runs rgit itself inside another directory (a submodule's working tree). Returns whether
/// it succeeded; with `quiet`, its output is discarded.
// Used by the `submodule` command to clone into, check out in and recurse into submodules.
pub fn run_rgit_in(dir: &Path, args: &[&str], quiet: bool) -> Result<bool> {
    let mut command = Command::new(std::env::current_exe()?);
    command.current_dir(dir).args(args);
    if quiet {
        command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    }
    Ok(command.status()?.success())
}

/// A short name for a commit of the repository at `path`, for `submodule status`: a tag
/// (`v1`) or branch (`heads/main`, `remotes/origin/main`) that points exactly at it, else
/// the abbreviated hash.
// Used by `submodule status`.
pub fn describe_submodule_commit(path: &str, hash: &str) -> String {
    let Some(git_dir) = submodule_git_dir(path) else { return hash[..7].to_string() };
    let mut refs: Vec<(String, String)> = Vec::new();
    if let Ok(packed) = fs::read_to_string(git_dir.join("packed-refs")) {
        refs.extend(packed.lines().filter(|l| !l.starts_with('#') && !l.starts_with('^')).filter_map(|l| l.split_once(' ')).map(|(h, n)| (n.to_string(), h.to_string())));
    }
    let mut stack = vec![git_dir.join("refs")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            } else if let (Ok(value), Ok(relative)) = (fs::read_to_string(&entry_path), entry_path.strip_prefix(&git_dir)) {
                refs.push((relative.to_string_lossy().replace('\\', "/"), value.trim().to_string()));
            }
        }
    }
    refs.sort();
    for prefix in ["refs/tags/", "refs/heads/", "refs/remotes/"] {
        if let Some((name, _)) = refs.iter().find(|(name, value)| name.starts_with(prefix) && value == hash) {
            return if prefix == "refs/tags/" { name[prefix.len()..].to_string() } else { name["refs/".len()..].to_string() };
        }
    }
    hash[..7].to_string()
}

/// The working-tree side of a submodule in a diff: `Subproject commit <its HEAD>`, or
/// `None` when no repository is checked out there.
// Used by `diff` and the raw diff commands.
pub fn submodule_worktree_bytes(path: &str) -> Option<Vec<u8>> {
    submodule_head(path).map(|head| format!("Subproject commit {}\n", head).into_bytes())
}

/// The submodules the path arguments select (all of them without arguments); an argument
/// that names no submodule ends the command with git's pathspec error.
// Used by the `submodule` command.
pub fn selected_submodules(paths: &[String]) -> Result<Vec<SubmoduleInfo>> {
    let modules = read_gitmodules()?;
    if paths.is_empty() {
        return Ok(modules);
    }
    let mut selected = Vec::new();
    for path in paths {
        let wanted = path.trim_end_matches('/');
        match modules.iter().find(|m| m.path == wanted) {
            Some(module) => selected.push(module.clone()),
            None => {
                eprintln!("error: pathspec '{}' did not match any file(s) known to git", path);
                crate::ui::exit(1);
            }
        }
    }
    Ok(selected)
}

/// Stages `.gitmodules` as it is on disk.
// Used by `submodule add`.
pub fn stage_gitmodules() -> Result<()> {
    let content = fs::read(".gitmodules")?;
    let hash = hash_content("blob", &content);
    write_object("blob", &content)?;
    let mut entries = read_index().unwrap_or_default();
    entries.retain(|e| e.path != ".gitmodules");
    entries.push(build_entry(".gitmodules", hash, &fs::metadata(".gitmodules")?));
    write_index(&mut entries)
}

