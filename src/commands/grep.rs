use std::fs;
use std::path::Path;
use anyhow::Result;
use crate::helpers::{build_grep_matcher, matches_pathspec, read_object, resolve_commit_from_source, resolve_tree_from_source, search_content};
use crate::index::read_index;
use crate::objects::{GrepFileResult, GrepOptions};

/// `rgit grep [-i] [-v] [-n] [-c] [-l|-L] [-F] [--cached] [-e <pattern>] [<pattern>] [<rev>...] [-- <path>...]`
///
/// Searches for patterns in tracked files or historical revisions.
pub fn grep(pattern: String, revisions: Vec<String>, path_specs: Vec<String>, options: GrepOptions) -> Result<()> {
    let matcher = build_grep_matcher(&pattern, options.fixed_strings, options.ignore_case)?;
    let mut results: Vec<GrepFileResult> = Vec::new();

    if revisions.is_empty() {
        let entries = read_index().unwrap_or_default();
        for entry in entries {
            if !matches_pathspec(&entry.path, &path_specs) {
                continue;
            }
            if entry.mode == 0o160000 {
                continue;
            }

            let content = if options.cached {
                match read_object(&hex::encode(entry.hash)) {
                    Ok((_, bytes)) => bytes,
                    Err(_) => continue,
                }
            } else {
                let path = Path::new(&entry.path);
                if !path.is_file() {
                    continue;
                }
                match fs::read(path) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                }
            };

            let res = search_content(&entry.path, &content, &matcher, &options);
            results.push(res);
        }
    } else {
        for rev in &revisions {
            let commit_hash = resolve_commit_from_source(rev)?;
            let tree_map = resolve_tree_from_source(&commit_hash)?;

            for (path, (hash, mode)) in tree_map {
                if !matches_pathspec(&path, &path_specs) {
                    continue;
                }
                if mode == 0o160000 {
                    continue;
                }

                let (_, content) = match read_object(&hex::encode(hash)) {
                    Ok(res) => res,
                    Err(_) => continue,
                };

                let display_path = format!("{}:{}", rev, path);
                let res = search_content(&display_path, &content, &matcher, &options);
                results.push(res);
            }
        }
    }

    let mut total_matches = 0;

    for res in results {
        let count = res.matches.len();
        if options.files_with_matches {
            if count > 0 {
                println!("{}", res.display_path);
                total_matches += 1;
            }
        } else if options.files_without_match {
            if count == 0 {
                println!("{}", res.display_path);
                total_matches += 1;
            }
        } else if options.count {
            if count > 0 || !options.invert_match {
                if res.is_binary && count > 0 {
                    println!("Binary file {} matches", res.display_path);
                } else {
                    println!("{}:{}", res.display_path, count);
                }
                if count > 0 {
                    total_matches += count;
                }
            }
        } else {
            if res.is_binary && count > 0 {
                println!("Binary file {} matches", res.display_path);
                total_matches += 1;
            } else {
                for (line_num, line) in res.matches {
                    if options.line_number {
                        println!("{}:{}:{}", res.display_path, line_num, line);
                    } else {
                        println!("{}:{}", res.display_path, line);
                    }
                    total_matches += 1;
                }
            }
        }
    }

    if total_matches == 0 {
        std::process::exit(1);
    }

    Ok(())
}
