//! Helpers of `rgit changelog`: parsing conventional-commit messages, finding tags, and
//! rendering a version's Markdown section.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use anyhow::Result;
use regex::Regex;

use crate::core::types::ParsedCommit;
use crate::history::changelog::types::Entry;

/// The sections, in order: commit type and heading. Types not listed go under "Other changes".
pub const SECTIONS: &[(&str, &str)] = &[
    ("feat", "Features"),
    ("fix", "Bug fixes"),
    ("perf", "Performance"),
    ("refactor", "Refactoring"),
    ("docs", "Documentation"),
    ("test", "Tests"),
    ("build", "Build and CI"),
    ("ci", "Build and CI"),
    ("style", "Style"),
    ("chore", "Chores"),
    ("revert", "Reverts"),
];

/// Reads a commit message as a conventional commit: `type(scope)!: description`, and a
/// `BREAKING CHANGE:` / `BREAKING-CHANGE:` footer. Anything else is an entry without a type.
// Used by `entries`.
pub fn parse_entry(commit: &ParsedCommit) -> Entry {
    static HEADER: OnceLock<Regex> = OnceLock::new();
    let re = HEADER.get_or_init(|| Regex::new(r"^([A-Za-z]+)(?:\(([^)]+)\))?(!)?:\s+(.+)$").unwrap());
    let subject = commit.message.lines().next().unwrap_or("").trim().to_string();
    let footer = commit.message.lines().find_map(|l| l.strip_prefix("BREAKING CHANGE:").or_else(|| l.strip_prefix("BREAKING-CHANGE:")).map(|t| t.trim().to_string()));
    let (kind, scope, text, bang) = match re.captures(&subject) {
        Some(c) => (Some(c[1].to_ascii_lowercase()), c.get(2).map(|m| m.as_str().to_string()), c[4].to_string(), c.get(3).is_some()),
        None => (None, None, subject.clone(), false),
    };
    let breaking = footer.or_else(|| bang.then(String::new));
    Entry { kind, scope, text, breaking, hash: commit.hash[..7].to_string(), author: commit.author.name.clone() }
}

/// The entries of the non-merge commits reachable from `tip` but not from `from` (everything
/// when None), newest first, with the newest commit's author time.
// Used by the `changelog` command.
pub fn entries(tip: &str, from: Option<&str>) -> Result<(Vec<Entry>, Option<i64>)> {
    let mut revs = vec![tip.to_string()];
    if let Some(f) = from {
        revs.push(format!("^{}", f));
    }
    let opts = crate::history::rev_list::engine::parse_revision_args(&revs)?;
    let (walked, _) = crate::history::rev_list::engine::select_rev_list_commits(&opts)?;
    let newest = walked.first().map(|w| w.commit.author.timestamp);
    let list = walked.into_iter().filter(|w| w.commit.parents.len() <= 1).map(|w| parse_entry(&w.commit)).collect();
    Ok((list, newest))
}

/// Every tag with the commit it points to (annotated tags peeled), oldest commit first.
// Used by the `changelog` command.
pub fn tags_by_date() -> Result<Vec<(String, String, i64)>> {
    let mut out = Vec::new();
    for tag in crate::core::refs::list_tags()? {
        if let Ok(hash) = crate::core::rev_parse::resolve_rev(&format!("refs/tags/{}^{{commit}}", tag)) {
            let when = crate::core::commit::parse_commit_object(&hash).map(|c| c.committer.timestamp).unwrap_or(0);
            out.push((tag, hash, when));
        }
    }
    out.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));
    Ok(out)
}

/// The newest tag whose commit `tip` has (the fewest commits away), if any; with `skip_tip`,
/// not a tag on `tip` itself.
// Used by the `changelog` command.
pub fn latest_tag(tip: &str, skip_tip: bool) -> Result<Option<(String, String)>> {
    let mut best: Option<(usize, String, String)> = None;
    for (tag, hash, _) in tags_by_date()? {
        if skip_tip && hash == tip {
            continue;
        }
        if hash == tip || crate::core::commit::is_reachable(tip, &hash)? {
            let distance = crate::ui::narrate::commits_between(Some(&hash), tip);
            if best.as_ref().is_none_or(|(d, _, _)| distance < *d) {
                best = Some((distance, tag, hash));
            }
        }
    }
    Ok(best.map(|(_, t, h)| (t, h)))
}

/// One version's Markdown: `## <version> (<date>)`, then breaking changes, then one list per
/// type in `SECTIONS` order, then "Other changes", then the contributors.
// Used by the `changelog` command.
pub fn render_section(version: &str, date: Option<String>, list: &[Entry]) -> String {
    let mut out = format!("## {}{}\n", version, date.map(|d| format!(" ({})", d)).unwrap_or_default());
    if list.is_empty() {
        out.push_str("\nNo changes.\n");
        return out;
    }
    let line = |e: &Entry, text: &str| match &e.scope {
        Some(scope) => format!("- **{}:** {} (`{}`)\n", scope, text, e.hash),
        None => format!("- {} (`{}`)\n", text, e.hash),
    };
    let breaking: Vec<&Entry> = list.iter().filter(|e| e.breaking.is_some()).collect();
    if !breaking.is_empty() {
        out.push_str("\n### Breaking changes\n\n");
        for e in breaking {
            let note = e.breaking.as_deref().filter(|n| !n.is_empty()).map(|n| format!("{}: {}", e.text, n)).unwrap_or_else(|| e.text.clone());
            out.push_str(&line(e, &note));
        }
    }
    let mut groups: BTreeMap<usize, (&str, Vec<&Entry>)> = BTreeMap::new();
    for e in list {
        let slot = e.kind.as_deref().and_then(|k| SECTIONS.iter().position(|(t, _)| *t == k));
        let (index, heading) = match slot {
            // `build` and `ci` share a heading: file both under the first one.
            Some(i) => {
                let heading = SECTIONS[i].1;
                (SECTIONS.iter().position(|(_, h)| *h == heading).unwrap_or(i), heading)
            }
            None => (SECTIONS.len(), "Other changes"),
        };
        groups.entry(index).or_insert((heading, Vec::new())).1.push(e);
    }
    for (_, (heading, items)) in groups {
        out.push_str(&format!("\n### {}\n\n", heading));
        for e in items {
            out.push_str(&line(e, &e.text));
        }
    }
    let mut authors: Vec<&str> = list.iter().map(|e| e.author.as_str()).collect();
    authors.sort();
    authors.dedup();
    out.push_str(&format!("\nContributors: {}\n", authors.join(", ")));
    out
}
