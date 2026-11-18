use anyhow::{Context, Result};
use regex::{Regex, RegexBuilder};
use crate::objects::{GrepFileResult, GrepOptions};

/// Builds a compiled `Regex` for the pattern, taking case-insensitivity and fixed-strings flags into account.
// Used by the `grep` command.
pub fn build_grep_matcher(pattern: &str, fixed_strings: bool, ignore_case: bool) -> Result<Regex> {
    let pat = if fixed_strings {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };

    RegexBuilder::new(&pat)
        .case_insensitive(ignore_case)
        .build()
        .with_context(|| format!("fatal: invalid regex '{}'", pattern))
}

/// Checks whether raw file bytes look like a binary file (contains null bytes in the first 8KB).
// Used by `grep` to handle binary files gracefully.
pub fn is_binary_content(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|&b| b == 0)
}

/// Checks whether a relative file path matches any of the given pathspec filters or globs.
// Used by `grep` to filter target search paths.
pub fn matches_pathspec(path: &str, path_specs: &[String]) -> bool {
    if path_specs.is_empty() {
        return true;
    }
    for spec in path_specs {
        let clean_spec = spec.trim_start_matches("./");
        if path.starts_with(clean_spec) || path == clean_spec {
            return true;
        }
        if crate::helpers::gitignore::glob_match(clean_spec, path) {
            return true;
        }
        if !clean_spec.contains('/') {
            if let Some(file_name) = std::path::Path::new(path).file_name().and_then(|f| f.to_str()) {
                if crate::helpers::gitignore::glob_match(clean_spec, file_name) {
                    return true;
                }
            }
        }
    }
    false
}

/// Searches a byte slice for matches using a compiled regex and returns a `GrepFileResult`.
// Used by the `grep` command.
pub fn search_content(
    display_path: &str,
    content: &[u8],
    matcher: &Regex,
    options: &GrepOptions,
) -> GrepFileResult {
    let is_binary = is_binary_content(content);
    let mut matches = Vec::new();

    if is_binary {
        let text = String::from_utf8_lossy(content);
        let has_match = matcher.is_match(&text);
        let passes = if options.invert_match { !has_match } else { has_match };
        if passes {
            matches.push((0, String::new()));
        }
    } else {
        let text = String::from_utf8_lossy(content);
        for (idx, line) in text.lines().enumerate() {
            let line_num = idx + 1;
            let is_match = matcher.is_match(line);
            let passes = if options.invert_match { !is_match } else { is_match };
            if passes {
                matches.push((line_num, line.to_string()));
                if options.files_with_matches {
                    break;
                }
            }
        }
    }

    GrepFileResult {
        display_path: display_path.to_string(),
        is_binary,
        matches,
    }
}
