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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_grep_matcher() {
        let regex = build_grep_matcher("hello.*world", false, false).unwrap();
        assert!(regex.is_match("hello beautiful world"));
        assert!(!regex.is_match("HELLO BEAUTIFUL WORLD"));

        let case_insensitive = build_grep_matcher("hello", false, true).unwrap();
        assert!(case_insensitive.is_match("HELLO"));

        let fixed = build_grep_matcher("a+b", true, false).unwrap();
        assert!(fixed.is_match("a+b"));
        assert!(!fixed.is_match("aaaaab"));
    }

    #[test]
    fn test_matches_pathspec() {
        assert!(matches_pathspec("src/main.rs", &[]));
        assert!(matches_pathspec("src/main.rs", &["src/".to_string()]));
        assert!(matches_pathspec("src/main.rs", &["*.rs".to_string()]));
        assert!(!matches_pathspec("src/main.rs", &["tests/".to_string()]));
    }

    #[test]
    fn test_is_binary_content() {
        assert!(!is_binary_content(b"hello world\n"));
        assert!(is_binary_content(b"hello\0world\n"));
    }

    #[test]
    fn test_search_content() {
        let options = GrepOptions {
            ignore_case: false,
            invert_match: false,
            line_number: true,
            count: false,
            files_with_matches: false,
            files_without_match: false,
            fixed_strings: false,
            cached: false,
        };
        let matcher = build_grep_matcher("apple", false, false).unwrap();
        let content = b"one apple\ntwo bananas\nthree apples\n";
        let res = search_content("test.txt", content, &matcher, &options);

        assert!(!res.is_binary);
        assert_eq!(res.matches.len(), 2);
        assert_eq!(res.matches[0], (1, "one apple".to_string()));
        assert_eq!(res.matches[1], (3, "three apples".to_string()));
    }
}

