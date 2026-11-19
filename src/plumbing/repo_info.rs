pub mod engine;

use anyhow::Result;

use crate::plumbing::repo_info::engine::{format_structure_table, repo_info_value, repo_structure_counts, REPO_INFO_KEYS, REPO_USAGE};

/// `rgit repo info [--format=(keyvalue|nul) | -z] [--all | <key>...]` /
/// `rgit repo structure [--format=(table|keyvalue|nul) | -z]`
///
/// Reports facts about the repository in a script-friendly form (the command git added in
/// 2.52). `repo info` prints the requested keys as `key=value` lines: `layout.bare`,
/// `layout.shallow`, `object.format` and `references.format`; `--all` prints every key, and
/// an unknown key is an error after the valid ones are printed. `repo structure` counts the
/// refs by kind (branches, tags, remote-tracking branches, others) and the objects reachable
/// from them by type (commits, trees, blobs, tags), shown as a table by default or as
/// `references.branches.count=3`-style lines with `--format=keyvalue`. `--format=nul` (or
/// `-z`) writes each key, a newline, the value and a NUL byte. Exit status: 0 success, 1 an
/// unknown key, 128 outside a repository, 129 usage.
pub fn repo(args: Vec<String>) -> Result<()> {
    let usage = || -> ! {
        eprint!("{}", REPO_USAGE);
        crate::ui::exit(129);
    };
    let Some((subcommand, rest)) = args.split_first() else { usage() };
    if subcommand == "-h" || rest.iter().any(|a| a == "-h") {
        print!("{}", REPO_USAGE);
        crate::ui::exit(129);
    }
    if !crate::core::repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }

    let mut format: Option<String> = None;
    let mut all = false;
    let mut keys: Vec<&String> = Vec::new();
    for arg in rest {
        match arg.as_str() {
            "-z" => format = Some("nul".to_string()),
            "--all" => all = true,
            _ if arg.starts_with("--format=") => format = Some(arg["--format=".len()..].to_string()),
            _ if arg.starts_with('-') => {
                eprintln!("error: unknown option `{}'", arg.trim_start_matches('-'));
                usage();
            }
            _ => keys.push(arg),
        }
    }
    let print = |format: &str, key: &str, value: &str| {
        if format == "nul" {
            print!("{}\n{}\0", key, value);
        } else {
            println!("{}={}", key, value);
        }
    };

    match subcommand.as_str() {
        "info" => {
            let format = format.unwrap_or_else(|| "keyvalue".to_string());
            if !matches!(format.as_str(), "keyvalue" | "nul") {
                eprintln!("fatal: invalid format '{}'", format);
                crate::ui::exit(128);
            }
            if all && !keys.is_empty() {
                eprintln!("fatal: --all and <key> cannot be used together");
                crate::ui::exit(128);
            }
            let wanted: Vec<String> = if all { REPO_INFO_KEYS.iter().map(|k| k.to_string()).collect() } else { keys.iter().map(|k| k.to_string()).collect() };
            let mut failed = false;
            for key in wanted {
                match repo_info_value(&key) {
                    Some(value) => print(&format, &key, &value),
                    None => {
                        eprintln!("error: key '{}' not found", key);
                        failed = true;
                    }
                }
            }
            if failed {
                crate::ui::exit(1);
            }
        }
        "structure" => {
            if all || !keys.is_empty() {
                usage();
            }
            let format = format.unwrap_or_else(|| "table".to_string());
            let counts = repo_structure_counts()?;
            match format.as_str() {
                "table" => print!("{}", format_structure_table(&counts)),
                "keyvalue" | "nul" => counts.iter().for_each(|(key, count)| print(&format, key, &count.to_string())),
                other => {
                    eprintln!("fatal: invalid format '{}'", other);
                    crate::ui::exit(128);
                }
            }
        }
        _ => usage(),
    }
    Ok(())
}
