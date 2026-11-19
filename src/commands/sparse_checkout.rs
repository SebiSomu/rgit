use std::fs;
use std::io::{self, Read};
use anyhow::Result;

use crate::core::config::{set_in, LOCAL_CONFIG_PATH};
use crate::helpers::sparse::{cone_directories, cone_patterns, is_cone_pattern_file, path_in_sparse_checkout, read_sparse_patterns, reapply_sparse_checkout, sparse_checkout_enabled, sparse_cone_mode, sparse_patterns_path, SPARSE_CHECKOUT_USAGE};
use crate::core::index::read_index;
use crate::core::repo;

/// `rgit sparse-checkout (init | set | add | list | reapply | disable | check-rules)`
///
/// Limits the working tree to part of the tracked files. The other files stay in the index
/// and in history, marked skip-worktree, but are not written to disk; every command treats
/// them as present and unchanged.
/// - `set <dir>...` chooses the directories (cone mode, the default): all files at the top
///   level, everything below the given directories, and the files directly inside their
///   parent directories. With `--no-cone` the arguments are `.gitignore`-style patterns
///   instead (`!` excludes). `--stdin` reads them from standard input.
/// - `add <dir>...` adds to the current selection; `list` prints it.
/// - `init` turns sparse checkout on with only the top-level files; `disable` turns it off
///   and restores every file; `reapply` applies the patterns again.
/// - `check-rules` reads paths from standard input and prints those the patterns select.
/// A file that should disappear but has local changes is kept and reported. The patterns
/// live in `.git/info/sparse-checkout`, the switches in `core.sparseCheckout` and
/// `core.sparseCheckoutCone`, the bits in an index of version 3 — all as in git. After
/// `checkout`, `switch`, `merge`, `reset` and the other commands that rewrite the working
/// tree, rgit applies the patterns again by itself. The sparse index format is not
/// supported. Exit status: 0, 128 on errors, 129 for a usage error.
pub fn sparse_checkout(args: Vec<String>) -> Result<()> {
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        crate::ui::exit(code);
    };
    let Some(action) = args.first().map(String::as_str) else { die(SPARSE_CHECKOUT_USAGE, 129) };
    if action == "-h" {
        print!("{}", SPARSE_CHECKOUT_USAGE);
        crate::ui::exit(129);
    }
    let (mut cone, mut stdin, mut nul, mut skip_checks) = (None, false, false, false);
    let mut rules_file: Option<String> = None;
    let mut values: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        match arg {
            "--cone" => cone = Some(true),
            "--no-cone" => cone = Some(false),
            "--stdin" => stdin = true,
            "-z" => nul = true,
            "--skip-checks" => skip_checks = true,
            "--sparse-index" => die("fatal: the sparse index format is not supported by rgit\n", 128),
            "--no-sparse-index" | "--end-of-options" => {}
            "--rules-file" => {
                rules_file = args.get(i).cloned();
                i += 1;
            }
            other if other.starts_with("--rules-file=") => rules_file = Some(other["--rules-file=".len()..].to_string()),
            "--" => {
                values.extend(args[i..].iter().cloned());
                break;
            }
            other if other.starts_with("--") => die(&format!("error: unknown option `{}'\n{}", &other[2..], SPARSE_CHECKOUT_USAGE), 129),
            other => values.push(other.to_string()),
        }
    }
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git\n", 128);
    }
    if stdin {
        let mut input = String::new();
        io::stdin().lock().read_to_string(&mut input)?;
        values.extend(input.lines().map(str::to_string).filter(|l| !l.is_empty()));
    }
    let config = repo::path(LOCAL_CONFIG_PATH);
    let patterns_path = sparse_patterns_path();
    let write_patterns = |text: &str| -> Result<()> {
        if let Some(dir) = patterns_path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&patterns_path, text)?;
        Ok(())
    };
    // Applies the pattern file and reports files that had to stay.
    let apply = || -> Result<()> {
        let patterns = read_sparse_patterns();
        let left = reapply_sparse_checkout(Some(&patterns))?;
        if !left.is_empty() {
            eprintln!("warning: The following paths are not up to date and were left despite sparse patterns:");
            for path in &left {
                eprintln!("\t{}", path);
            }
            eprintln!("\nAfter fixing the above paths, you may want to run `rgit sparse-checkout reapply`.");
        }
        Ok(())
    };
    // In cone mode the arguments must be directories, not tracked files.
    let check_directories = |dirs: &[String]| {
        if skip_checks {
            return;
        }
        let index = read_index().unwrap_or_default();
        for dir in dirs {
            let name = dir.trim_matches('/');
            if index.iter().any(|e| e.path == name) {
                die(&format!("fatal: '{}' is not a directory; to treat it as a directory anyway, rerun with --skip-checks\n", dir), 128);
            }
        }
    };

    match action {
        "init" => {
            let use_cone = cone.unwrap_or(true);
            set_in(&config, "core.sparseCheckout", "true")?;
            set_in(&config, "core.sparseCheckoutCone", if use_cone { "true" } else { "false" })?;
            if !patterns_path.exists() {
                write_patterns("/*\n!/*/\n")?;
            }
            apply()?;
        }
        "set" | "add" => {
            let was_enabled = sparse_checkout_enabled();
            let use_cone = cone.unwrap_or_else(|| if was_enabled { sparse_cone_mode() } else { true });
            if action == "add" && !was_enabled {
                die("fatal: no sparse-checkout to add to\n", 128);
            }
            if values.is_empty() && !stdin {
                die(&format!("fatal: specify directories rather than patterns\n{}", SPARSE_CHECKOUT_USAGE), 129);
            }
            let existing = if action == "add" { read_sparse_patterns() } else { Vec::new() };
            let text = if use_cone {
                if values.iter().any(|v| v.starts_with('/')) {
                    die("fatal: specify directories rather than patterns (no leading slash)\n", 128);
                }
                check_directories(&values);
                let mut dirs = cone_directories(&existing);
                dirs.extend(values.iter().cloned());
                cone_patterns(&dirs)
            } else {
                let mut lines = existing;
                lines.extend(values.iter().cloned());
                lines.iter().map(|l| format!("{}\n", l)).collect()
            };
            set_in(&config, "core.sparseCheckout", "true")?;
            set_in(&config, "core.sparseCheckoutCone", if use_cone { "true" } else { "false" })?;
            write_patterns(&text)?;
            apply()?;
        }
        "list" => {
            if !sparse_checkout_enabled() {
                die("fatal: this worktree is not sparse\n", 128);
            }
            let patterns = read_sparse_patterns();
            if sparse_cone_mode() && is_cone_pattern_file(&patterns) {
                for dir in cone_directories(&patterns) {
                    println!("{}", dir);
                }
            } else {
                for pattern in patterns {
                    println!("{}", pattern);
                }
            }
        }
        "reapply" => {
            if !sparse_checkout_enabled() {
                die("fatal: must be in a sparse-checkout to reapply sparsity patterns\n", 128);
            }
            if let Some(use_cone) = cone {
                set_in(&config, "core.sparseCheckoutCone", if use_cone { "true" } else { "false" })?;
            }
            apply()?;
        }
        "disable" => {
            set_in(&config, "core.sparseCheckout", "false")?;
            set_in(&config, "core.sparseCheckoutCone", "false")?;
            reapply_sparse_checkout(None)?;
        }
        "check-rules" => {
            let patterns: Vec<String> = match &rules_file {
                Some(file) => {
                    let text = fs::read_to_string(file).unwrap_or_else(|e| die(&format!("fatal: unable to open '{}': {}\n", file, e), 128));
                    let lines: Vec<String> = text.lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
                    if cone.unwrap_or(true) { cone_patterns(&lines).lines().map(str::to_string).collect() } else { lines }
                }
                None => read_sparse_patterns(),
            };
            let mut input = String::new();
            io::stdin().lock().read_to_string(&mut input)?;
            let separator = if nul { '\0' } else { '\n' };
            for path in input.split(separator).filter(|p| !p.is_empty()) {
                if path_in_sparse_checkout(path, &patterns) {
                    print!("{}{}", path, separator);
                }
            }
        }
        other => die(&format!("error: unknown subcommand: `{}'\n{}", other, SPARSE_CHECKOUT_USAGE), 129),
    }
    Ok(())
}
