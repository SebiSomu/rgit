use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::helpers::objects::read_object;
use crate::objects::ShortlogOptions;
use crate::refs;

/// A single author entry produced by `collect_shortlog`: the display name/email
/// together with the subject lines of every commit attributed to that author.
// Used by the `shortlog` command for rendering.
pub struct AuthorEntry {
    /// The key used for deduplication/display (name, or "name <email>" with `-e`).
    pub display: String,
    /// Subject lines of commits attributed to this author, in walk order (newest first).
    pub subjects: Vec<String>,
}

/// Strips the trailing `<timestamp> <tz>` suffix from a raw author header value.
/// Returns a sub-slice that ends before the Unix timestamp.
// Used by `parse_author_key`.
fn strip_author_timestamp(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] != b' ' {
        end -= 1;
    }
    if end > 0 {
        end -= 1;
    }
    let mark = end;
    while end > 0 && bytes[end - 1] != b' ' {
        end -= 1;
    }
    if end > 0 {
        end -= 1;
    }
    let timestamp_candidate = &s[end + 1..mark];
    if !timestamp_candidate.is_empty() && timestamp_candidate.chars().all(|c| c.is_ascii_digit()) {
        s[..end + 1].trim_end()
    } else {
        s
    }
}

/// Parses an `author` header value (`Name <email> timestamp tz`) into the
/// display key used for grouping: either the name only or "Name <email>".
// Used by `walk_commits_dedup`.
fn parse_author_key(author_raw: &str, use_email: bool) -> String {
    let trimmed = strip_author_timestamp(author_raw);
    if !use_email {
        if let Some(angle) = trimmed.find('<') {
            trimmed[..angle].trim().to_string()
        } else {
            trimmed.trim().to_string()
        }
    } else {
        if let Some(close) = trimmed.rfind('>') {
            trimmed[..=close].trim().to_string()
        } else {
            trimmed.trim().to_string()
        }
    }
}

/// Peels an object to a commit hash (follows annotated tag objects).
/// Returns `None` if the object cannot be resolved to a commit.
// Used by `collect_shortlog` for `--all` tag handling.
fn peel_to_commit(mut hash: String) -> Result<Option<String>> {
    for _ in 0..10 {
        let (obj_type, content) = match read_object(&hash) {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };
        if obj_type == "commit" {
            return Ok(Some(hash));
        }
        if obj_type == "tag" {
            let text = String::from_utf8_lossy(&content);
            if let Some(next) = text.lines().find_map(|l| l.strip_prefix("object ")) {
                hash = next.to_string();
                continue;
            }
        }
        return Ok(None);
    }
    Ok(None)
}

/// Walks commits from `start_hash` via BFS, deduplicating globally with the shared `seen` set.
/// Returns `(display_key, subject)` pairs in discovery order.
// Used by `collect_shortlog`.
fn walk_commits_dedup(start_hash: &str, use_email: bool, seen: &mut HashSet<String>) -> Result<Vec<(String, String)>> {
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back(start_hash.to_string());
    let mut out: Vec<(String, String)> = Vec::new();

    while let Some(hash) = queue.pop_front() {
        if seen.contains(&hash) {
            continue;
        }
        seen.insert(hash.clone());

        let (obj_type, content) = match read_object(&hash) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if obj_type != "commit" {
            continue;
        }

        let text = String::from_utf8_lossy(&content);
        let mut author_line: Option<String> = None;
        let mut parents: Vec<String> = Vec::new();
        let mut in_message = false;
        let mut subject: Option<String> = None;

        for line in text.lines() {
            if in_message {
                if subject.is_none() && !line.trim().is_empty() {
                    subject = Some(line.to_string());
                }
            } else if line.is_empty() {
                in_message = true;
            } else if let Some(a) = line.strip_prefix("author ") {
                author_line = Some(a.to_string());
            } else if let Some(p) = line.strip_prefix("parent ") {
                parents.push(p.to_string());
            }
        }

        let author_raw = author_line.unwrap_or_default();
        let display_key = parse_author_key(&author_raw, use_email);
        let subject_text = subject.unwrap_or_default();
        out.push((display_key, subject_text));

        for p in parents {
            queue.push_back(p);
        }
    }

    Ok(out)
}

/// Collects and sorts shortlog entries according to `opts`.
///
/// - Walks the commit graph from HEAD, a given `opts.rev`, or all refs (with `opts.all`).
/// - Groups commits by author name (or name+email with `opts.email`).
/// - Sorts alphabetically by default, or by commit count (descending) with `opts.numbered`.
// Used by the `shortlog` command.
pub fn collect_shortlog(opts: &ShortlogOptions) -> Result<Vec<AuthorEntry>> {
    let mut start_hashes: Vec<String> = Vec::new();

    if opts.all {
        let branches = refs::list_branches().context("Failed to list branches for --all")?;
        for b in branches {
            if let Some(h) = refs::read_ref(&format!("refs/heads/{}", b))? {
                start_hashes.push(h);
            }
        }
        if let Ok(tags) = refs::list_tags() {
            for t in tags {
                if let Some(h) = refs::read_ref(&format!("refs/tags/{}", t))? {
                    if let Some(c) = peel_to_commit(h)? {
                        start_hashes.push(c);
                    }
                }
            }
        }
    } else {
        let hash = if let Some(rev) = &opts.rev {
            crate::helpers::rev_parse::resolve_rev(rev)
                .with_context(|| format!("bad revision '{}'", rev))?
        } else {
            refs::resolve_head_commit()?
                .ok_or_else(|| anyhow::anyhow!("fatal: your current branch has no commits yet"))?
        };
        start_hashes.push(hash);
    }

    let mut all_entries: Vec<(String, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for start in start_hashes {
        let entries = walk_commits_dedup(&start, opts.email, &mut seen)?;
        all_entries.extend(entries);
    }

    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (author, subject) in all_entries {
        map.entry(author).or_default().push(subject);
    }

    let mut result: Vec<AuthorEntry> = map
        .into_iter()
        .map(|(display, subjects)| AuthorEntry { display, subjects })
        .collect();

    if opts.numbered {
        result.sort_by(|a, b| {
            b.subjects.len().cmp(&a.subjects.len())
                .then_with(|| a.display.cmp(&b.display))
        });
    } else {
        result.sort_by(|a, b| a.display.to_lowercase().cmp(&b.display.to_lowercase()));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_author_key_name_only() {
        let raw = "Alice Smith <alice@example.com> 1234567890 +0000";
        assert_eq!(parse_author_key(raw, false), "Alice Smith");
    }

    #[test]
    fn test_parse_author_key_with_email() {
        let raw = "Alice Smith <alice@example.com> 1234567890 +0000";
        assert_eq!(parse_author_key(raw, true), "Alice Smith <alice@example.com>");
    }

    #[test]
    fn test_parse_author_key_no_email_field() {
        let raw = "Bob 1234567890 +0000";
        assert_eq!(parse_author_key(raw, false), "Bob");
    }

    #[test]
    fn test_strip_author_timestamp() {
        let s = "Alice <a@b.com> 1700000000 +0300";
        assert_eq!(strip_author_timestamp(s), "Alice <a@b.com>");
    }

    #[test]
    fn test_strip_author_timestamp_no_timestamp() {
        // If the string doesn't end with a well-formed timestamp, return it unchanged
        let s = "No Timestamp Here";
        assert_eq!(strip_author_timestamp(s), "No Timestamp Here");
    }
}