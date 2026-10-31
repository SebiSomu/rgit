use std::fs;
use std::path::Path;
use anyhow::Context;
use crate::helpers::check_ref_format::check_refname_format;
use crate::helpers::reflog::{delete_reflog, record_symref_update};
use crate::objects::{RawRefValue, ResolvedRef, SymbolicRefOptions};
use crate::refs::read_packed_ref;
use crate::repo;

/// Longest chain of symbolic refs followed before giving up (git's `SYMREF_MAXDEPTH`).
const SYMREF_MAX_DEPTH: usize = 5;

/// git's usage text for `symbolic-ref`, printed for `-h` and after option errors (exit 129).
pub const SYMBOLIC_REF_USAGE: &str = "usage: rgit symbolic-ref [-m <reason>] <name> <ref>
   or: rgit symbolic-ref [-q] [--short] [--no-recurse] <name>
   or: rgit symbolic-ref --delete [-q] <name>

    -q, --[no-]quiet      suppress error message for non-symbolic (detached) refs
    -d, --[no-]delete     delete symbolic ref
    --[no-]short          shorten ref output
    --[no-]recurse        recursively dereference (default)
    -m <reason>           reason of the update
";

/// Parses `symbolic-ref` arguments like git's option parser: switches can be bundled (`-qd`),
/// `-m` takes the rest of its word or the next argument, options may follow the names, and
/// everything after `--` is a name. Errors carry the usage text and mean exit 129.
// Used by `symbolic-ref`.
pub fn parse_symbolic_ref_args(args: &[String]) -> anyhow::Result<SymbolicRefOptions> {
    let mut opts = SymbolicRefOptions::default();
    let usage_error = |message: String| anyhow::anyhow!("{}\n{}", message, SYMBOLIC_REF_USAGE);

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--" => {
                opts.args.extend(args[i + 1..].iter().cloned());
                break;
            }
            "--quiet" => opts.quiet = true,
            "--no-quiet" => opts.quiet = false,
            "--delete" => opts.delete = true,
            "--no-delete" => opts.delete = false,
            "--short" => opts.short = true,
            "--no-short" => opts.short = false,
            "--recurse" => opts.recurse = true,
            "--no-recurse" => opts.recurse = false,
            _ if arg.starts_with("--") => {
                let name = arg[2..].split('=').next().unwrap_or("");
                return Err(usage_error(format!("error: unknown option `{}'", name)));
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                let switches = &arg[1..];
                for (pos, c) in switches.char_indices() {
                    match c {
                        'q' => opts.quiet = true,
                        'd' => opts.delete = true,
                        'm' => {
                            let rest = &switches[pos + 1..];
                            if !rest.is_empty() {
                                opts.message = Some(rest.to_string());
                            } else if let Some(next) = args.get(i + 1) {
                                opts.message = Some(next.clone());
                                i += 1;
                            } else {
                                return Err(usage_error("error: switch `m' requires a value".to_string()));
                            }
                            break;
                        }
                        other => return Err(usage_error(format!("error: unknown switch `{}'", other))),
                    }
                }
            }
            _ => opts.args.push(arg.to_string()),
        }
        i += 1;
    }
    Ok(opts)
}

/// Says whether a ref name is safe to write even when it fails the full format check, like
/// git's `refname_is_safe`: names under `refs/` must be already-normalized relative paths
/// (no empty, `.` or `..` components, not starting with `/` or `.`), and anything else must
/// be all uppercase letters and underscores (`HEAD`, `ORIG_HEAD`, `FOO`).
// Used by `write_symbolic_ref`.
pub fn is_refname_safe(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("refs/") {
        return !rest.is_empty()
            && !rest.starts_with('.')
            && rest.split('/').all(|c| !c.is_empty() && c != "." && c != "..");
    }
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
}

/// Reads one ref without following symbolic refs: a loose file under `.git` (a directory or a
/// missing file counts as no ref), else its `packed-refs` entry. Loose contents are either
/// `ref: <target>` or a 40-hex hash optionally followed by whitespace; anything else is broken.
// Used by `resolve_ref_unsafe`.
pub fn read_raw_ref_value(name: &str) -> RawRefValue {
    let path = repo::path(format!(".git/{}", name));
    if path.is_file() {
        let Ok(content) = fs::read_to_string(&path) else { return RawRefValue::Broken };
        if let Some(target) = content.strip_prefix("ref:") {
            let target = target.trim();
            return if target.is_empty() { RawRefValue::Broken } else { RawRefValue::Symbolic(target.to_string()) };
        }
        let hash = content.get(..40).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
        let tail_ok = content[hash.map_or(0, |h| h.len())..].chars().next().is_none_or(char::is_whitespace);
        return match hash {
            Some(h) if tail_ok => RawRefValue::Hash(h.to_ascii_lowercase()),
            _ => RawRefValue::Broken,
        };
    }
    if path.is_dir() {
        return RawRefValue::Missing;
    }
    match read_packed_ref(name) {
        Some(hash) => RawRefValue::Hash(hash),
        None => RawRefValue::Missing,
    }
}

/// Resolves a ref name the way git's `resolve_ref_unsafe` does: follows `ref: ` pointers (at
/// most 5 deep, or just one step when `recurse` is false) and returns the name the walk ends
/// on. A ref that doesn't exist resolves to itself with no hash, unless `reading` is set, in
/// which case it is `None`. Bad names, broken ref files and chains that are too deep are `None`.
// Used by `symbolic-ref` (reading, setting and deleting).
pub fn resolve_ref_unsafe(name: &str, recurse: bool, reading: bool) -> Option<ResolvedRef> {
    if !check_refname_format(name, true, false) {
        return None;
    }
    let mut current = name.to_string();
    let mut is_symref = false;
    for _ in 0..SYMREF_MAX_DEPTH {
        match read_raw_ref_value(&current) {
            RawRefValue::Missing if reading => return None,
            RawRefValue::Missing => return Some(ResolvedRef { name: current, hash: None, is_symref }),
            RawRefValue::Broken => return None,
            RawRefValue::Hash(hash) => return Some(ResolvedRef { name: current, hash: Some(hash), is_symref }),
            RawRefValue::Symbolic(target) => {
                is_symref = true;
                if !recurse {
                    return Some(ResolvedRef { name: target, hash: None, is_symref });
                }
                if !check_refname_format(&target, true, false) {
                    return None;
                }
                current = target;
            }
        }
    }
    None
}

/// Lists the names in `.git/packed-refs` (skipping the header and `^` peeled lines).
// Used by `check_ref_creation_conflicts`.
fn packed_ref_names() -> Vec<String> {
    let Ok(packed) = fs::read_to_string(repo::path(".git/packed-refs")) else { return Vec::new() };
    packed
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .filter_map(|line| line.split_once(' ').map(|(_, name)| name.to_string()))
        .collect()
}

/// Says whether a directory tree contains any file at all.
// Used by `check_ref_creation_conflicts`.
fn dir_has_files(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else { return false };
    entries.flatten().any(|e| {
        let path = e.path();
        if path.is_dir() { dir_has_files(&path) } else { true }
    })
}

/// Checks that a ref file can be created at `name`, with git's "cannot lock ref" errors: no
/// existing ref (loose or packed) may be a path prefix of it or have it as a prefix, and a
/// directory in its place is only allowed (and removed) when it holds no files.
// Used by `write_symbolic_ref` and `update-ref` (creating refs).
pub fn check_ref_creation_conflicts(name: &str) -> anyhow::Result<()> {
    let lock_error = |reason: String| anyhow::anyhow!("error: cannot lock ref '{}': {}", name, reason);
    let packed = packed_ref_names();

    let parts: Vec<&str> = name.split('/').collect();
    for end in 1..parts.len() {
        let prefix = parts[..end].join("/");
        if repo::path(format!(".git/{}", prefix)).is_file() || packed.contains(&prefix) {
            return Err(lock_error(format!("'{}' exists; cannot create '{}'", prefix, name)));
        }
    }

    let nested = format!("{}/", name);
    if let Some(existing) = packed.iter().find(|p| p.starts_with(&nested)) {
        return Err(lock_error(format!("'{}' exists; cannot create '{}'", existing, name)));
    }

    let path = repo::path(format!(".git/{}", name));
    if path.is_dir() {
        if dir_has_files(&path) {
            return Err(lock_error(format!(
                "there is a non-empty directory '{}' blocking reference '{}'",
                repo::to_git_path_string(&path),
                name
            )));
        }
        fs::remove_dir_all(&path).with_context(|| format!("Failed to remove empty directory {}", path.display()))?;
    }
    Ok(())
}

/// Points the ref `name` at `target` by writing `ref: <target>` into its own file (never
/// dereferencing `name`, so `HEAD` itself changes, not the branch it is on). When the target
/// exists, an entry from the ref's old value to the target's commit is added to `name`'s
/// reflog with the `-m` message (empty without one), like git. Errors are git's
/// `error: ...` messages (exit 1).
// Used by `symbolic-ref <name> <ref>`.
pub fn write_symbolic_ref(name: &str, target: &str, message: Option<&str>) -> anyhow::Result<()> {
    if !is_refname_safe(name) {
        anyhow::bail!("error: refusing to update ref with bad name '{}'", name);
    }
    if name.starts_with("refs/") {
        check_ref_creation_conflicts(name)?;
    }

    let old_hash = resolve_ref_unsafe(name, true, false).and_then(|r| r.hash);

    let path = repo::path(format!(".git/{}", name));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(&path, format!("ref: {}\n", target))
        .with_context(|| format!("error: unable to write symref for {}", name))?;

    if let Some(new_hash) = resolve_ref_unsafe(target, true, true).and_then(|r| r.hash) {
        record_symref_update(name, old_hash.as_deref(), &new_hash, message.unwrap_or(""));
    }
    Ok(())
}

/// Deletes the symbolic ref `name` itself (not what it points at) along with its reflog, and
/// removes directories under `.git/refs` that become empty.
// Used by `symbolic-ref --delete`.
pub fn delete_symbolic_ref(name: &str) -> anyhow::Result<()> {
    let path = repo::path(format!(".git/{}", name));
    fs::remove_file(&path).with_context(|| format!("error: unable to delete ref '{}'", name))?;
    delete_reflog(name);
    crate::refs::prune_empty_ref_dirs(&path);
    Ok(())
}
