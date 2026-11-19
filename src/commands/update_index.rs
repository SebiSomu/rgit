use std::collections::BTreeMap;
use std::io::{self, Read};
use anyhow::Result;

use crate::helpers::index_plumbing::{add_cache_entry, bare_index_entry, index_mode_from, normalize_index_path, refresh_index, update_index_path};
use crate::helpers::objects::{flatten_tree, tree_hash_of_commit};
use crate::helpers::update_ref::unquote_c_style;
use crate::index::{read_index, write_index, IndexEntry};
use crate::objects::UpdateIndexFlags;
use crate::repo;

const UPDATE_INDEX_USAGE: &str = "usage: rgit update-index [<options>] [--] [<file>...]\n\n    -q                    continue refresh even when index needs update\n    --[no-]add            do not ignore new files\n    --[no-]replace        let files replace directories and vice-versa\n    --[no-]remove         notice files missing from worktree\n    --refresh             refresh stat information\n    --really-refresh      like --refresh\n    --cacheinfo <mode>,<object>,<path>\n                          add the specified entry to the index\n    --chmod (+|-)x        override the executable bit of the listed files\n    --[no-]info-only      add to index only; do not add content to object database\n    --[no-]force-remove   remove named paths even if present in worktree\n    -z                    with --stdin: input lines are terminated by null bytes\n    --stdin               read list of paths to be updated from standard input\n    --index-info          add entries from standard input to the index\n    -g, --again           only update entries that differ from HEAD\n    --[no-]ignore-missing ignore files missing from worktree\n    --[no-]verbose        report actions to standard output\n\n";

/// Prints a message and exits with the given status.
fn die(message: &str, code: i32) -> ! {
    eprintln!("{}", message);
    crate::ui::exit(code);
}

/// Handles one path the way `update-index <file>` does, including `--force-remove`,
/// `--chmod` and `--verbose` reporting.
fn process_path(entries: &mut Vec<IndexEntry>, arg: &str, flags: &UpdateIndexFlags) {
    let path = match normalize_index_path(arg) {
        Ok(Some(path)) => path,
        Ok(None) => {
            eprintln!("Ignoring path {}", arg);
            return;
        }
        Err(message) => die(&message, 128),
    };
    if let Some(skip) = flags.skip_worktree {
        // Only the bit changes; the file is not looked at.
        match entries.iter_mut().find(|e| e.path == path) {
            Some(entry) => entry.skip_worktree = skip,
            None => die(&format!("fatal: Unable to mark file {}", path), 128),
        }
        return;
    }
    if flags.force_remove {
        entries.retain(|e| e.path != path);
        if flags.verbose {
            println!("remove '{}'", path);
        }
        return;
    }
    if let Err(message) = update_index_path(entries, &path, flags) {
        die(&format!("{}\nfatal: Unable to process path {}", message, path), 128);
    }
    if flags.verbose {
        println!("add '{}'", path);
    }
    if let Some(executable) = flags.chmod {
        let sign = if executable { '+' } else { '-' };
        match entries.iter_mut().find(|e| e.path == path && e.mode & 0o170000 == 0o100000) {
            Some(entry) => entry.mode = if executable { 0o100755 } else { 0o100644 },
            None => die(&format!("fatal: git update-index: cannot chmod {}x '{}'", sign, path), 128),
        }
        if flags.verbose {
            println!("chmod {}x '{}'", sign, path);
        }
    }
}

/// Adds a `--cacheinfo` entry; `Err` is git's `error:` text.
fn add_cacheinfo(entries: &mut Vec<IndexEntry>, mode: u32, hash: &str, path: &str, flags: &UpdateIndexFlags) -> Result<(), String> {
    let Ok(Some(path)) = normalize_index_path(path) else { return Err(format!("error: Invalid path '{}'", path)) };
    let mut bytes = [0u8; 20];
    hex::decode_to_slice(hash, &mut bytes).map_err(|_| format!("error: Invalid object name '{}'", hash))?;
    add_cache_entry(entries, bare_index_entry(&path, bytes, index_mode_from(mode)), flags)?;
    if flags.verbose {
        println!("add '{}'", path);
    }
    Ok(())
}

/// Applies `--index-info` input: lines of `<mode> SP <hash> TAB <path>`,
/// `<mode> SP <type> SP <hash> TAB <path>` or `<mode> SP <hash> SP <stage> TAB <path>`;
/// mode 0 removes the path.
fn read_index_info(entries: &mut Vec<IndexEntry>, input: &[u8], nul_terminated: bool, flags: &UpdateIndexFlags) {
    let flags = UpdateIndexFlags { add: true, replace: true, remove: true, ..flags.clone() };
    let terminator = if nul_terminated { 0u8 } else { b'\n' };
    for line in input.split(|b| *b == terminator).filter(|l| !l.is_empty()) {
        let shown = String::from_utf8_lossy(line).to_string();
        let malformed = || -> ! { die(&format!("fatal: malformed index info {}", shown), 128) };
        let Some(tab) = line.iter().position(|b| *b == b'\t') else { malformed() };
        let head = String::from_utf8_lossy(&line[..tab]).to_string();
        let fields: Vec<&str> = head.split(' ').collect();
        let Ok(mode) = u32::from_str_radix(fields[0], 8) else { malformed() };
        let is_hash = |s: &str| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit());
        let (hash, stage) = match fields.as_slice() {
            [_, hash] if is_hash(hash) => (*hash, 0),
            [_, hash, stage] if is_hash(hash) && matches!(*stage, "0" | "1" | "2" | "3") => (*hash, stage.parse::<u32>().unwrap_or(0)),
            [_, _type, hash] if is_hash(hash) => (*hash, 0),
            _ => malformed(),
        };
        let mut name = line[tab + 1..].to_vec();
        if !nul_terminated && name.first() == Some(&b'"') {
            match unquote_c_style(&name, 0) {
                Some((unquoted, end)) if end == name.len() => name = unquoted,
                _ => die("fatal: git update-index: bad quoting of path name", 128),
            }
        }
        let path = String::from_utf8_lossy(&name).to_string();
        if mode == 0 {
            if let Ok(Some(path)) = normalize_index_path(&path) {
                entries.retain(|e| e.path != path);
            }
            continue;
        }
        if stage != 0 {
            die(&format!("fatal: git update-index: unable to update {}: rgit's index has no conflict stages", path), 128);
        }
        if let Err(message) = add_cacheinfo(entries, mode, &hash.to_lowercase(), &path, &flags) {
            die(&format!("{}\nfatal: git update-index: unable to update {}", message, path), 128);
        }
    }
}

/// `rgit update-index [<options>] [--] [<file>...]`
///
/// Low-level editing of the index. Options and files are handled in the order given, so a
/// switch only affects what follows it. A `<file>` is re-read from the working tree: its
/// content is hashed, stored and staged if it changed. New files need `--add`; a file that
/// no longer exists needs `--remove` (or `--force-remove`, which drops the path from the
/// index even when the file is there); `--replace` lets a file replace a directory of
/// entries and the reverse; `--info-only` stages the hash without storing the blob.
/// `--cacheinfo <mode>,<object>,<path>` (or the three as separate arguments) stages an
/// entry straight from an object id; `--index-info` reads such entries from standard input
/// in `ls-tree` / `ls-files -s` form (mode 0 removes a path); `--stdin` reads file names
/// (`-z`: NUL-terminated). `--chmod=(+|-)x` sets the executable bit of the files after it.
/// `--refresh` re-checks every entry against its file, updates the cached stat data and
/// prints `<path>: needs update` for changed or missing files, exiting 1 (`-q`: silently
/// and successfully; `--ignore-missing`: missing files are fine). `-g` / `--again` updates
/// the entries that differ from `HEAD`. `--verbose` reports each action.
/// `--skip-worktree` / `--no-skip-worktree` set or clear the skip-worktree bit of the files
/// after them (the bit `sparse-checkout` uses). Not supported, because rgit's index stores
/// no other per-entry flags or conflict stages: `--assume-unchanged`, `--unresolve`, stage numbers in `--index-info`,
/// and the split-index / untracked-cache / fsmonitor switches. Exit status: 0, 1 when
/// `--refresh` found entries needing an update, 128 on errors, 129 for a usage error.
pub fn update_index(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        print!("{}", UPDATE_INDEX_USAGE);
        crate::ui::exit(129);
    }
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git", 128);
    }
    let mut entries = read_index()?;
    let mut flags = UpdateIndexFlags::default();
    let (mut nul, mut read_stdin, mut index_info, mut needs_update) = (false, false, false, false);
    let usage_error = |message: String| -> ! {
        eprint!("{}\n{}", message, UPDATE_INDEX_USAGE);
        crate::ui::exit(129);
    };

    let mut i = 0;
    let mut options_done = false;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if options_done || !arg.starts_with('-') || arg == "-" {
            process_path(&mut entries, arg, &flags);
            continue;
        }
        match arg {
            "--" => options_done = true,
            "-q" => flags.quiet = true,
            "--add" => flags.add = true,
            "--no-add" => flags.add = false,
            "--replace" => flags.replace = true,
            "--no-replace" => flags.replace = false,
            "--remove" => flags.remove = true,
            "--no-remove" => flags.remove = false,
            "--force-remove" => flags.force_remove = true,
            "--no-force-remove" => flags.force_remove = false,
            "--info-only" => flags.info_only = true,
            "--no-info-only" => flags.info_only = false,
            "--ignore-missing" => flags.ignore_missing = true,
            "--no-ignore-missing" => flags.ignore_missing = false,
            "--verbose" => flags.verbose = true,
            "--no-verbose" => flags.verbose = false,
            "--unmerged" | "--no-unmerged" | "--ignore-submodules" | "--no-ignore-submodules" | "--force-write-index" | "--no-force-write-index" | "--clear-resolve-undo" => {}
            "-z" => nul = true,
            "--stdin" => read_stdin = true,
            "--index-info" => index_info = true,
            "--refresh" | "--really-refresh" => {
                let stale = refresh_index(&mut entries, flags.ignore_missing);
                if !flags.quiet {
                    for path in &stale {
                        println!("{}: needs update", path);
                    }
                    needs_update |= !stale.is_empty();
                }
            }
            "-g" | "--again" => {
                let mut head_tree = BTreeMap::new();
                if let Some(head) = crate::refs::resolve_head_commit()? {
                    flatten_tree(&tree_hash_of_commit(&head)?, "", &mut head_tree)?;
                }
                let differing: Vec<String> = entries.iter().filter(|e| head_tree.get(&e.path) != Some(&(e.hash, e.mode))).map(|e| e.path.clone()).collect();
                for path in differing {
                    process_path(&mut entries, &path, &flags);
                }
            }
            "--cacheinfo" => {
                let expects = || -> ! {
                    eprintln!("error: option 'cacheinfo' expects <mode>,<sha1>,<path>");
                    crate::ui::exit(129);
                };
                let Some(first) = args.get(i).cloned() else { expects() };
                let is_hash = |s: &str| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit());
                let parts: Vec<&str> = first.splitn(3, ',').collect();
                let (mode, hash, path) = if parts.len() == 3 && u32::from_str_radix(parts[0], 8).is_ok() && is_hash(parts[1]) {
                    i += 1;
                    (parts[0].to_string(), parts[1].to_string(), parts[2].to_string())
                } else if !first.contains(',') && args.len() >= i + 3 {
                    i += 3;
                    (first.clone(), args[i - 2].clone(), args[i - 1].clone())
                } else {
                    expects()
                };
                let added = match u32::from_str_radix(&mode, 8) {
                    Ok(mode) if is_hash(&hash) => add_cacheinfo(&mut entries, mode, &hash.to_lowercase(), &path, &flags),
                    _ => Err(String::new()),
                };
                if let Err(message) = added {
                    let prefix = if message.is_empty() { String::new() } else { format!("{}\n", message) };
                    die(&format!("{}fatal: git update-index: --cacheinfo cannot add {}", prefix, path), 128);
                }
            }
            other if other.starts_with("--chmod") => {
                let value = match other.strip_prefix("--chmod=") {
                    Some(value) => value.to_string(),
                    None if other == "--chmod" => {
                        i += 1;
                        args.get(i - 1).cloned().unwrap_or_default()
                    }
                    None => usage_error(format!("error: unknown option '{}'", &other[2..])),
                };
                flags.chmod = match value.as_str() {
                    "+x" => Some(true),
                    "-x" => Some(false),
                    _ => {
                        eprintln!("error: option 'chmod' expects \"+x\" or \"-x\"");
                        crate::ui::exit(129);
                    }
                };
            }
            "--skip-worktree" => flags.skip_worktree = Some(true),
            "--no-skip-worktree" => flags.skip_worktree = Some(false),
            "--assume-unchanged" | "--no-assume-unchanged" | "--fsmonitor-valid" | "--no-fsmonitor-valid" | "--unresolve" => {
                die(&format!("fatal: {} is not supported: rgit's index stores no per-entry flags or conflict stages", arg), 128);
            }
            other if other.starts_with("--") => {
                let name = other[2..].split('=').next().unwrap_or("");
                let known_unsupported = ["index-version", "show-index-version", "split-index", "untracked-cache", "test-untracked-cache", "force-untracked-cache", "fsmonitor", "ignore-skip-worktree-entries"];
                if known_unsupported.contains(&name.trim_start_matches("no-")) {
                    die(&format!("fatal: {} is not supported by rgit's index format", other), 128);
                }
                usage_error(format!("error: unknown option '{}'", &other[2..]));
            }
            other => usage_error(format!("error: unknown switch `{}'", other.chars().nth(1).unwrap_or('-'))),
        }
    }

    if read_stdin || index_info {
        if read_stdin && index_info {
            die("fatal: --stdin and --index-info cannot be used together", 128);
        }
        let mut input = Vec::new();
        io::stdin().lock().read_to_end(&mut input)?;
        if index_info {
            read_index_info(&mut entries, &input, nul, &flags);
        } else {
            let terminator = if nul { 0u8 } else { b'\n' };
            for line in input.split(|b| *b == terminator).filter(|l| !l.is_empty()) {
                let mut name = line.to_vec();
                if !nul && name.first() == Some(&b'"') {
                    match unquote_c_style(&name, 0) {
                        Some((unquoted, end)) if end == name.len() => name = unquoted,
                        _ => die("fatal: line is badly quoted", 128),
                    }
                }
                process_path(&mut entries, &String::from_utf8_lossy(&name), &flags);
            }
        }
    }

    write_index(&mut entries)?;
    if needs_update {
        crate::ui::exit(1);
    }
    Ok(())
}
