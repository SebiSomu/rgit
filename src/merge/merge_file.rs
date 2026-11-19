pub mod engine;
pub mod types;

use std::io::Write;
use anyhow::Result;

use crate::core::config::get_config_value;
use crate::merge::merge_file::engine::parse_merge_file_args;
use crate::merge::merge_tree::engine::merge_file_lines_with;
use crate::core::object_store::{read_object, write_object};
use crate::core::rev_parse::resolve_rev;
use crate::merge::{merge_file::types::MergeFileOptions, types::MergeConflictStyle};
use crate::core::repo;

/// `rgit merge-file [-p] [-q] [--diff3 | --zdiff3] [--ours | --theirs | --union]
/// [--marker-size=<n>] [--object-id] [-L <label>]... <current> <base> <other>`
///
/// Three-way merge of files: applies to `<current>` the changes that lead from `<base>` to
/// `<other>`, and overwrites `<current>` with the result (`-p`: prints it instead). Regions
/// both sides changed differently become conflicts, marked `<<<<<<<` / `=======` /
/// `>>>>>>>` with the file names (or the `-L` labels, in the order current, base, other).
/// `--diff3` / `--zdiff3` also show the base version (default from `merge.conflictStyle`);
/// `--ours` / `--theirs` / `--union` resolve conflicts by taking that side (or both) instead
/// of marking them. With `--object-id` the three arguments name blobs and the result is
/// written to the object store and its id printed (or its content, with `-p`). Works
/// outside a repository on plain files. Exit status: the number of conflicts (at most 127),
/// 0 for a clean merge, 255 on errors, 129 for a usage error.
pub fn merge_file(args: Vec<String>) -> Result<()> {
    let in_repo = repo::exists();
    let config_style = if in_repo { get_config_value("merge.conflictstyle").ok().flatten() } else { None };
    let style = match config_style.as_deref() {
        Some("diff3") => MergeConflictStyle::Diff3,
        Some("zdiff3") => MergeConflictStyle::Zdiff3,
        _ => MergeConflictStyle::Merge,
    };
    let opts = match parse_merge_file_args(&args, style) {
        Ok(opts) => opts,
        Err(message) => {
            if message.starts_with("usage:") && args.iter().any(|a| a == "-h") {
                print!("{}", message);
            } else {
                eprint!("{}", message);
            }
            crate::ui::exit(129);
        }
    };
    if opts.object_id && !in_repo {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }

    let mut contents: Vec<String> = Vec::new();
    for name in &opts.files {
        let bytes = if opts.object_id {
            let Ok(hash) = resolve_rev(name) else {
                eprintln!("error: object '{}' does not exist", name);
                crate::ui::exit(255);
            };
            read_object(&hash)?.1
        } else {
            match std::fs::read(name) {
                Ok(bytes) => bytes,
                Err(e) => {
                    let reason = if e.kind() == std::io::ErrorKind::NotFound { "No such file or directory".to_string() } else { e.to_string() };
                    eprintln!("error: Could not stat {}: {}", name, reason);
                    crate::ui::exit(255);
                }
            }
        };
        if bytes[..bytes.len().min(8000)].contains(&0) {
            eprintln!("error: Cannot merge binary files: {}", name);
            crate::ui::exit(255);
        }
        contents.push(String::from_utf8_lossy(&bytes).to_string());
    }

    let label = |i: usize| opts.labels.get(i).cloned().unwrap_or_else(|| opts.files[i].clone());
    let merge_opts = MergeFileOptions { ours_label: label(0), base_label: label(1), theirs_label: label(2), marker_size: opts.marker_size, style: opts.style, favor: opts.favor, join_non_alnum: true };
    let result = merge_file_lines_with(&contents[1], &contents[0], &contents[2], &merge_opts);

    if opts.object_id && !opts.stdout {
        println!("{}", write_object("blob", &result.content)?);
    } else if opts.stdout {
        std::io::stdout().lock().write_all(&result.content)?;
    } else if let Err(e) = std::fs::write(&opts.files[0], &result.content) {
        eprintln!("error: Could not open {} for writing: {}", opts.files[0], e);
        crate::ui::exit(255);
    }
    let conflicts = result.conflicts.len().min(127);
    if conflicts > 0 {
        crate::ui::exit(conflicts as i32);
    }
    Ok(())
}
