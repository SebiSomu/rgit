use std::io::{self, Read};
use std::path::PathBuf;
use anyhow::Result;

use crate::helpers::index_plumbing::{normalize_index_path, worktree_state, write_entry_to_worktree, write_merge_temp_file};
use crate::helpers::objects::read_object;
use crate::helpers::update_ref::unquote_c_style;
use crate::index::{build_entry, read_index, write_index, IndexEntry};
use crate::objects::IndexFileState;
use crate::repo;

const CHECKOUT_INDEX_USAGE: &str = "usage: rgit checkout-index [<options>] [--] [<file>...]\n\n    -a, --[no-]all        check out all files in the index\n    -f, --[no-]force      force overwrite of existing files\n    -q, --[no-]quiet      no warning for existing files and files not in index\n    -n, --no-create       don't checkout new files\n    --create              opposite of --no-create\n    -u, --[no-]index      update stat information in the index file\n    -z                    paths are separated with NUL character\n    --[no-]stdin          read list of paths from the standard input\n    --[no-]temp           write the content to temporary files\n    --[no-]prefix <string>\n                          when creating files, prepend <string>\n    --stage (1|2|3|all)   copy out the files from named stage\n\n";

/// `rgit checkout-index [-a] [-f] [-q] [-n] [-u] [--prefix=<string>] [--temp]
/// [--stage=<n>|all] [-z] [--stdin] [--] [<file>...]`
///
/// Copies files from the index to the working tree: the named files, or every entry with
/// `-a`. A file that already exists and differs from the index is left alone and reported
/// (`<file> already exists, no checkout`, exit 1) unless `-f` overwrites it; a file that
/// already matches is skipped silently. `-n` only refreshes existing files and creates
/// none; `-q` silences the reports; `-u` records the stat data of the files written in the
/// index. `--prefix=<string>` writes to `<string><path>` instead (`--prefix=out/` exports
/// the index into a directory; any file already there counts as existing). `--temp` writes
/// each file to a `.merge_file_XXXXXX` temporary file and prints `<temp> TAB <path>`.
/// `--stdin` reads the file names from standard input (`-z`: NUL-terminated). rgit's index
/// has no conflict stages, so `--stage=1..3` finds nothing and `--stage=all` prints nothing.
/// Exit status: 0, 1 when a file was skipped or is not in the index, 128 on fatal errors,
/// 129 for a usage error.
pub fn checkout_index(args: Vec<String>) -> Result<()> {
    let die = |message: &str, code: i32| -> ! {
        eprintln!("{}", message);
        crate::ui::exit(code);
    };
    let (mut all, mut force, mut quiet, mut no_create, mut update_stat, mut nul, mut read_stdin, mut temp) = (false, false, false, false, false, false, false, false);
    let mut prefix = String::new();
    let mut stage: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut options_done = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if options_done || !arg.starts_with('-') || arg == "-" {
            files.push(arg.to_string());
            continue;
        }
        let mut value_of = |name: &str| -> String {
            match arg.split_once('=') {
                Some((_, value)) => value.to_string(),
                None => {
                    i += 1;
                    match args.get(i - 1) {
                        Some(value) => value.clone(),
                        None => {
                            eprintln!("error: option `{}' requires a value", name);
                            crate::ui::exit(129);
                        }
                    }
                }
            }
        };
        match arg.split('=').next().unwrap_or(arg) {
            "--" => options_done = true,
            "-h" => {
                print!("{}", CHECKOUT_INDEX_USAGE);
                crate::ui::exit(129);
            }
            "-a" | "--all" => all = true,
            "--no-all" => all = false,
            "-f" | "--force" => force = true,
            "--no-force" => force = false,
            "-q" | "--quiet" => quiet = true,
            "--no-quiet" => quiet = false,
            "-n" | "--no-create" => no_create = true,
            "--create" | "--no-no-create" => no_create = false,
            "-u" | "--index" => update_stat = true,
            "--no-index" => update_stat = false,
            "-z" => nul = true,
            "--stdin" => read_stdin = true,
            "--no-stdin" => read_stdin = false,
            "--temp" => temp = true,
            "--no-temp" => temp = false,
            "--ignore-skip-worktree-bits" | "--no-ignore-skip-worktree-bits" => {}
            "--prefix" => prefix = value_of("prefix"),
            "--no-prefix" => prefix.clear(),
            "--stage" => {
                let value = value_of("stage");
                if !matches!(value.as_str(), "1" | "2" | "3" | "all") {
                    die("fatal: stage should be between 1 and 3 or all", 128);
                }
                stage = Some(value);
            }
            other if other.starts_with("--") => {
                eprint!("error: unknown option `{}'\n{}", &other[2..], CHECKOUT_INDEX_USAGE);
                crate::ui::exit(129);
            }
            other => {
                eprint!("error: unknown switch `{}'\n{}", other.chars().nth(1).unwrap_or('-'), CHECKOUT_INDEX_USAGE);
                crate::ui::exit(129);
            }
        }
    }
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git", 128);
    }
    if all && !files.is_empty() {
        die("fatal: git checkout-index: don't mix '--all' and explicit filenames", 128);
    }
    if read_stdin && !files.is_empty() {
        die("fatal: git checkout-index: don't mix '--stdin' and explicit filenames", 128);
    }
    if read_stdin && all {
        die("fatal: git checkout-index: don't mix '--all' and '--stdin'", 128);
    }
    if read_stdin {
        let mut input = Vec::new();
        io::stdin().lock().read_to_end(&mut input)?;
        let terminator = if nul { 0u8 } else { b'\n' };
        for line in input.split(|b| *b == terminator).filter(|l| !l.is_empty()) {
            let mut name = line.to_vec();
            if !nul && name.first() == Some(&b'"') {
                match unquote_c_style(&name, 0) {
                    Some((unquoted, end)) if end == name.len() => name = unquoted,
                    _ => die("fatal: line is badly quoted", 128),
                }
            }
            files.push(String::from_utf8_lossy(&name).to_string());
        }
    }

    let mut entries = read_index()?;
    let mut errors = 0;
    let mut refreshed: Vec<IndexEntry> = Vec::new();

    // The entries to check out, in order; a named file that is not in the index is reported.
    let mut wanted: Vec<IndexEntry> = Vec::new();
    if all {
        wanted = entries.clone();
    } else {
        for file in &files {
            let path = match normalize_index_path(file) {
                Ok(Some(path)) => path,
                Ok(None) => file.clone(),
                Err(message) => die(&message, 128),
            };
            match entries.iter().find(|e| e.path == path) {
                Some(entry) if matches!(stage.as_deref(), Some("1" | "2" | "3")) => {
                    if !quiet {
                        eprintln!("git checkout-index: {} does not exist at stage {}", entry.path, stage.as_deref().unwrap_or(""));
                    }
                    errors += 1;
                }
                Some(entry) => wanted.push(entry.clone()),
                None => {
                    if !quiet {
                        eprintln!("git checkout-index: {} is not in the cache", path);
                    }
                    errors += 1;
                }
            }
        }
    }
    // Only stage-0 entries exist: a specific stage has nothing, and "all" lists unmerged paths only.
    if stage.is_some() {
        wanted.clear();
    }

    for entry in &wanted {
        if temp {
            let content = read_object(&hex::encode(entry.hash))?.1;
            match write_merge_temp_file(&content) {
                Ok(name) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mode = if entry.mode & 0o111 != 0 { 0o755 } else { 0o644 };
                        let _ = std::fs::set_permissions(&name, std::fs::Permissions::from_mode(mode));
                    }
                    if nul {
                        print!("{}\t{}\0", name, entry.path);
                    } else {
                        println!("{}\t{}", name, entry.path);
                    }
                }
                Err(e) => die(&format!("fatal: unable to create temporary file: {}", e), 128),
            }
            continue;
        }

        let target = PathBuf::from(format!("{}{}", prefix, entry.path));
        let shown = format!("{}{}", prefix, entry.path);
        let exists = std::fs::symlink_metadata(&target).is_ok();
        if exists {
            // In place, a file that still matches the index needs nothing; anywhere else it is just in the way.
            if prefix.is_empty() && worktree_state(entry) == IndexFileState::Clean {
                continue;
            }
            if !force {
                if !quiet {
                    eprintln!("{} already exists, no checkout", shown);
                }
                errors += 1;
                continue;
            }
        } else if no_create {
            continue;
        }
        match write_entry_to_worktree(entry, &target, force) {
            Ok(()) => {
                if update_stat && prefix.is_empty() {
                    if let Ok(metadata) = std::fs::metadata(&target) {
                        let mut fresh = build_entry(&entry.path, entry.hash, &metadata);
                        fresh.mode = entry.mode;
                        refreshed.push(fresh);
                    }
                }
            }
            Err(message) if message.starts_with("fatal:") => die(&message, 128),
            Err(message) => {
                eprintln!("{}", message);
                errors += 1;
            }
        }
    }

    if !refreshed.is_empty() {
        for fresh in refreshed {
            if let Some(entry) = entries.iter_mut().find(|e| e.path == fresh.path) {
                *entry = fresh;
            }
        }
        write_index(&mut entries)?;
    }
    if errors > 0 {
        crate::ui::exit(1);
    }
    Ok(())
}
