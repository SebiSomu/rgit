use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use anyhow::Context;
use crate::helpers::check_ref_format::check_refname_format;
use crate::helpers::read_object;
use crate::helpers::rev_parse::peel_to_type;
use crate::helpers::wildmatch::wildmatch;
use crate::objects::{PackRefsOptions, PackedRefEntry};
use crate::refs::prune_empty_ref_dirs;
use crate::repo;

/// git's usage text for `pack-refs`, printed for `-h` and after option errors (exit 129).
pub const PACK_REFS_USAGE: &str = "usage: rgit pack-refs [--all] [--no-prune] [--include <pattern>] [--exclude <pattern>]

    --[no-]all            pack everything
    --[no-]prune          prune loose refs (default)
    --[no-]include <pattern>
                          references to include
    --[no-]exclude <pattern>
                          references to exclude
";

/// First line of a `packed-refs` file written by git (and now rgit): every annotated tag is
/// followed by its peeled value, and entries are sorted.
pub const PACKED_REFS_HEADER: &str = "# pack-refs with: peeled fully-peeled sorted \n";

/// Refs under these prefixes belong to one worktree and are never packed (git's
/// `parse_worktree_ref` check).
const PER_WORKTREE_PREFIXES: &[&str] = &["refs/bisect/", "refs/worktree/", "refs/rewritten/"];

/// Parses `pack-refs` arguments like git's option parser: `--include`/`--exclude` take their
/// pattern as `=<p>` or the next argument and accumulate (`--no-include`/`--no-exclude` clear
/// them). Positional arguments are not allowed. Errors carry the usage text (exit 129).
// Used by `pack-refs`.
pub fn parse_pack_refs_args(args: &[String]) -> anyhow::Result<PackRefsOptions> {
    let mut opts = PackRefsOptions::default();
    let usage_error = |message: String| anyhow::anyhow!("{}{}", message, PACK_REFS_USAGE);

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--all" => opts.all = true,
            "--no-all" => opts.all = false,
            "--prune" => opts.prune = true,
            "--no-prune" => opts.prune = false,
            "--no-include" => opts.includes.clear(),
            "--no-exclude" => opts.excludes.clear(),
            "--include" | "--exclude" => {
                let Some(value) = args.get(i + 1) else {
                    anyhow::bail!("error: option `{}' requires a value\n", &arg[2..]);
                };
                if arg == "--include" { opts.includes.push(value.clone()) } else { opts.excludes.push(value.clone()) }
                i += 1;
            }
            _ if arg.starts_with("--include=") => opts.includes.push(arg["--include=".len()..].to_string()),
            _ if arg.starts_with("--exclude=") => opts.excludes.push(arg["--exclude=".len()..].to_string()),
            _ if arg.starts_with("--") && arg.len() > 2 => {
                let name = arg[2..].split('=').next().unwrap_or("");
                return Err(usage_error(format!("error: unknown option `{}'\n", name)));
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                let switch = arg[1..].chars().next().unwrap_or('-');
                return Err(usage_error(format!("error: unknown switch `{}'\n", switch)));
            }
            _ => return Err(usage_error(String::new())),
        }
        i += 1;
    }
    Ok(opts)
}

/// Reads `.git/packed-refs` into a name → entry map (keeping `^` peeled lines with their ref),
/// and says whether the file declared itself `fully-peeled`. A missing file is empty.
// Used by `pack_refs_matching`.
pub fn read_packed_entries() -> (BTreeMap<String, PackedRefEntry>, bool) {
    let mut entries: BTreeMap<String, PackedRefEntry> = BTreeMap::new();
    let Ok(content) = fs::read_to_string(repo::path(".git/packed-refs")) else { return (entries, false) };

    let mut fully_peeled = false;
    let mut last: Option<String> = None;
    for line in content.lines() {
        if let Some(header) = line.strip_prefix("# pack-refs with:") {
            fully_peeled = header.split_whitespace().any(|trait_name| trait_name == "fully-peeled");
            continue;
        }
        if let Some(peeled) = line.strip_prefix('^') {
            if let Some(entry) = last.as_ref().and_then(|name| entries.get_mut(name)) {
                entry.peeled = Some(peeled.trim().to_string());
            }
            continue;
        }
        match line.split_once(' ') {
            Some((hash, name)) if hash.len() == 40 && !line.starts_with('#') => {
                entries.insert(name.to_string(), PackedRefEntry { hash: hash.to_string(), peeled: None });
                last = Some(name.to_string());
            }
            _ => last = None,
        }
    }
    (entries, fully_peeled)
}

/// Takes `.git/packed-refs.lock` (creating it, failing if it already exists) with git's
/// message for a lock held by another process.
// Used by `pack_refs_matching`.
pub fn lock_packed_refs() -> anyhow::Result<fs::File> {
    let lock = repo::path(".git/packed-refs.lock");
    match fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
        Ok(file) => Ok(file),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => anyhow::bail!(
            "fatal: Unable to create '{}': File exists.\n\n\
             Another git process seems to be running in this repository, e.g.\n\
             an editor opened by 'git commit'. Please make sure all processes\n\
             are terminated then try again. If it still fails, a git process\n\
             may have crashed in this repository earlier:\n\
             remove the file manually to continue.",
            repo::to_git_path_string(&repo::normalize(&lock))
        ),
        Err(e) => Err(e).with_context(|| format!("Unable to create '{}'", lock.display())),
    }
}

/// Writes `.git/packed-refs` from scratch into the held `packed-refs.lock` and renames it into
/// place, with git's header, entries in byte order, and a `^<peeled>` line after every
/// annotated tag.
// Used by `pack_refs_matching`.
pub fn write_packed_refs(mut lock: fs::File, entries: &BTreeMap<String, PackedRefEntry>) -> anyhow::Result<()> {
    let mut out = String::from(PACKED_REFS_HEADER);
    for (name, entry) in entries {
        out.push_str(&format!("{} {}\n", entry.hash, name));
        if let Some(peeled) = &entry.peeled {
            out.push_str(&format!("^{}\n", peeled));
        }
    }
    let lock_path = repo::path(".git/packed-refs.lock");
    let written = lock.write_all(out.as_bytes()).and_then(|()| lock.sync_all());
    drop(lock);
    if let Err(e) = written {
        let _ = fs::remove_file(&lock_path);
        return Err(e).context("Failed to write .git/packed-refs.lock");
    }
    fs::rename(&lock_path, repo::path(".git/packed-refs")).context("Failed to update .git/packed-refs")?;
    Ok(())
}

/// The object a packed entry for `hash` should list on its `^` line: what an annotated tag
/// peels to, `None` for anything else (or an object that can't be read).
// Used by `pack_refs_matching`.
fn peeled_value(hash: &str) -> Option<String> {
    match read_object(hash) {
        Ok((object_type, _)) if object_type == "tag" => peel_to_type(hash.to_string(), "").ok(),
        _ => None,
    }
}

/// Walks the loose refs under `.git/refs` in name order, the way git's loose-ref cache does:
/// names starting with `.` and `*.lock` files are skipped, and each ref comes with its file
/// contents (`None` when unreadable).
// Used by `pack_refs_matching`.
fn collect_loose_ref_files(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Option<String>>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') || file_name.ends_with(".lock") {
            continue;
        }
        let name = format!("{}/{}", prefix, file_name);
        let path = entry.path();
        if path.is_dir() {
            collect_loose_ref_files(&path, &name, out);
        } else {
            out.insert(name, fs::read_to_string(&path).ok());
        }
    }
}

/// Packs the loose refs selected by `opts` into `.git/packed-refs`, like git's files backend:
/// per-worktree refs, symbolic refs and refs with bad names or contents are never packed, a
/// ref whose object is missing is reported (`error: ... does not point to a valid object!`)
/// and left alone, `--exclude` patterns win, then `--include` patterns (`*` with `--all`,
/// `refs/tags/*` when none) decide. Entries already in `packed-refs` stay, packed values
/// replace older packed ones, and annotated tags get their peeled line. With `prune`, each
/// packed loose file is removed if it still holds the packed value, along with directories
/// that became empty (reflogs are kept).
// Used by `pack-refs`.
pub fn pack_refs_matching(opts: &PackRefsOptions) -> anyhow::Result<()> {
    let mut includes = opts.includes.clone();
    if opts.all {
        includes.push("*".to_string());
    }
    if includes.is_empty() {
        includes.push("refs/tags/*".to_string());
    }

    let lock = lock_packed_refs()?;
    let mut loose = BTreeMap::new();
    collect_loose_ref_files(&repo::path(".git/refs"), "refs", &mut loose);

    let mut to_pack: Vec<(String, String)> = Vec::new();
    for (name, content) in loose {
        if PER_WORKTREE_PREFIXES.iter().any(|p| name.starts_with(p)) || !check_refname_format(&name, true, false) {
            continue;
        }
        let Some(content) = content else { continue };
        let hash = content.get(..40).filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
        let Some(hash) = hash.filter(|_| content[40..].chars().next().is_none_or(char::is_whitespace)) else { continue };
        let hash = hash.to_ascii_lowercase();
        if read_object(&hash).is_err() {
            eprintln!("error: {} does not point to a valid object!", name);
            continue;
        }
        if opts.excludes.iter().any(|p| wildmatch(p, &name, 0)) || !includes.iter().any(|p| wildmatch(p, &name, 0)) {
            continue;
        }
        to_pack.push((name, hash));
    }

    let (mut entries, fully_peeled) = read_packed_entries();
    if !fully_peeled {
        for entry in entries.values_mut().filter(|e| e.peeled.is_none()) {
            entry.peeled = peeled_value(&entry.hash);
        }
    }
    for (name, hash) in &to_pack {
        entries.insert(name.clone(), PackedRefEntry { hash: hash.clone(), peeled: peeled_value(hash) });
    }
    write_packed_refs(lock, &entries)?;

    if opts.prune {
        for (name, hash) in &to_pack {
            let path = repo::path(format!(".git/{}", name));
            let unchanged = fs::read_to_string(&path).is_ok_and(|c| c.trim().eq_ignore_ascii_case(hash));
            if unchanged && fs::remove_file(&path).is_ok() {
                prune_empty_ref_dirs(&path);
            }
        }
    }
    Ok(())
}
