pub mod cli;
pub mod engine;
pub mod types;

use std::fs;
use std::path::Path;
use anyhow::Result;
use crate::history::grep::engine::{build_grep_matcher, matches_pathspec, search_content};
use crate::core::commit::{resolve_commit_from_source, resolve_tree_from_source};
use crate::core::object_store::read_object;
use crate::core::index::read_index;
use crate::history::grep::types::{GrepFileResult, GrepOptions};

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
    let paint = GrepColors::new(options.color);

    for res in results {
        let count = res.matches.len();
        if options.files_with_matches {
            if count > 0 {
                println!("{}", paint.file(&res.display_path));
                total_matches += 1;
            }
        } else if options.files_without_match {
            if count == 0 {
                println!("{}", paint.file(&res.display_path));
                total_matches += 1;
            }
        } else if options.count {
            if count > 0 {
                if res.is_binary && count > 0 {
                    println!("Binary file {} matches", res.display_path);
                } else {
                    println!("{}{}{}", paint.file(&res.display_path), paint.sep(), count);
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
                    let text = if options.invert_match { line.clone() } else { paint.matches(&line, &matcher) };
                    if options.line_number {
                        println!("{}{}{}{}{}", paint.file(&res.display_path), paint.sep(), paint.line_number(line_num), paint.sep(), text);
                    } else {
                        println!("{}{}{}", paint.file(&res.display_path), paint.sep(), text);
                    }
                    total_matches += 1;
                }
            }
        }
    }

    if total_matches == 0 {
        crate::ui::exit(1);
    }

    Ok(())
}

/// git's grep colors (`color.grep.<slot>`): file names magenta, `:` cyan, line numbers green,
/// matches bold red; nothing when colors are off.
// Used by `grep`.
struct GrepColors {
    on: bool,
    filename: String,
    separator: String,
    line_number: String,
    matched: String,
}

impl GrepColors {
    fn new(on: bool) -> GrepColors {
        use crate::core::color::{slot, BOLD_RED, CYAN, GREEN, MAGENTA};
        GrepColors {
            on,
            filename: slot("grep", "filename", MAGENTA),
            separator: slot("grep", "separator", CYAN),
            line_number: slot("grep", "lineNumber", GREEN),
            matched: match slot("grep", "matchSelected", "") {
                selected if selected.is_empty() => slot("grep", "match", BOLD_RED),
                selected => selected,
            },
        }
    }

    fn wrap(&self, text: &str, color: &str) -> String {
        if self.on && !color.is_empty() { format!("{}{}{}", color, text, crate::core::color::RESET) } else { text.to_string() }
    }

    fn file(&self, path: &str) -> String {
        self.wrap(path, &self.filename)
    }

    fn sep(&self) -> String {
        self.wrap(":", &self.separator)
    }

    fn line_number(&self, n: usize) -> String {
        self.wrap(&n.to_string(), &self.line_number)
    }

    /// `line` with every match painted.
    fn matches(&self, line: &str, matcher: &regex::Regex) -> String {
        if !self.on {
            return line.to_string();
        }
        let mut out = String::new();
        let mut last = 0;
        for m in matcher.find_iter(line).filter(|m| !m.as_str().is_empty()) {
            out.push_str(&line[last..m.start()]);
            out.push_str(&self.wrap(m.as_str(), &self.matched));
            last = m.end();
        }
        out.push_str(&line[last..]);
        out
    }
}
