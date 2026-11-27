pub mod types;

use std::collections::{BTreeMap, BTreeSet};

use crate::diff::plumbing::quote_c_path;
use crate::workspace::status::types::StatusOptions;

/// Quotes a path for `status --short` like git: C-style quoting (control and non-ASCII bytes,
/// `"` and `\`), and also wrapping a path with a space in double quotes.
// Used by `format_short_status`.
fn quote_status_path(path: &str) -> String {
    let quoted = quote_c_path(path);
    if quoted == path && path.contains(' ') {
        format!("\"{}\"", path)
    } else {
        quoted
    }
}

/// Collapses untracked files into their topmost untracked directory (`d/`) the way git's
/// default `--untracked-files=normal` shows them: a directory is collapsed when nothing under
/// it is tracked. Returns the sorted, de-duplicated entries.
// Used by `status` (long and short formats).
pub fn collapse_untracked(untracked: &[String], tracked: &BTreeSet<String>) -> Vec<String> {
    let mut tracked_dirs = BTreeSet::new();
    for path in tracked {
        let mut prefix = String::new();
        let mut parts: Vec<&str> = path.split('/').collect();
        parts.pop();
        for part in parts {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            tracked_dirs.insert(prefix.clone());
        }
    }
    let mut out = BTreeSet::new();
    for path in untracked {
        let parts: Vec<&str> = path.split('/').collect();
        let mut prefix = String::new();
        let mut shown = path.clone();
        for part in &parts[..parts.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if !tracked_dirs.contains(&prefix) {
                shown = format!("{}/", prefix);
                break;
            }
        }
        out.insert(shown);
    }
    out.into_iter().collect()
}

/// git's `## ` branch line for `status -sb`: `## main...origin/main [ahead 1, behind 2]`,
/// `## No commits yet on main`, or `## HEAD (no branch)`.
// Used by `status --short --branch`.
pub fn status_branch_header(head: &crate::core::refs::HeadState, unborn: bool) -> String {
    let branch = match head {
        crate::core::refs::HeadState::Detached(_) => return "## HEAD (no branch)".to_string(),
        crate::core::refs::HeadState::Branch(b) => b,
    };
    if unborn {
        return format!("## No commits yet on {}", branch);
    }
    let Some(upstream) = crate::references::for_each_ref::engine::upstream::branch_upstream(branch) else {
        return format!("## {}", branch);
    };
    let short = crate::references::for_each_ref::engine::upstream::shorten_unambiguous_ref(&upstream);
    let track = match crate::references::for_each_ref::engine::upstream::tracking_counts(&format!("refs/heads/{}", branch), &upstream) {
        None => " [gone]".to_string(),
        Some((0, 0)) => String::new(),
        Some((a, 0)) => format!(" [ahead {}]", a),
        Some((0, b)) => format!(" [behind {}]", b),
        Some((a, b)) => format!(" [ahead {}, behind {}]", a, b),
    };
    format!("## {}...{}{}", branch, short, track)
}

/// Renders `status --short` / `--porcelain`: one `XY path` line per changed path (index
/// column X against HEAD, worktree column Y against the index, `R  old -> new` for renames,
/// git's two-letter codes for unmerged paths), in path order, then `?? path` for untracked.
/// With `-z`, entries end in NUL, paths are not quoted and renames print `new\0old`.
// Used by `status -s` / `--porcelain`.
pub fn format_short_status(
    opts: &StatusOptions,
    header: Option<String>,
    staged: &[(char, String)],
    renamed: &[(char, String, String)],
    unstaged: &[(char, String)],
    unmerged: &BTreeMap<String, u8>,
    untracked: &[String],
) -> String {
    let end = if opts.nul { "\0" } else { "\n" };
    let show = |p: &str| if opts.nul { p.to_string() } else { quote_status_path(p) };
    // path -> (X, Y, rename source)
    let mut rows: BTreeMap<String, (char, char, Option<String>)> = BTreeMap::new();
    for (code, path) in staged {
        rows.entry(path.clone()).or_insert((' ', ' ', None)).0 = *code;
    }
    for (code, from, to) in renamed {
        let row = rows.entry(to.clone()).or_insert((' ', ' ', None));
        row.0 = *code;
        row.2 = Some(from.clone());
    }
    for (code, path) in unstaged {
        rows.entry(path.clone()).or_insert((' ', ' ', None)).1 = *code;
    }
    for (path, mask) in unmerged {
        let (x, y) = match mask {
            1 => ('D', 'D'),
            2 => ('A', 'U'),
            3 => ('U', 'D'),
            4 => ('U', 'A'),
            5 => ('D', 'U'),
            6 => ('A', 'A'),
            _ => ('U', 'U'),
        };
        rows.insert(path.clone(), (x, y, None));
    }
    let colors = ShortColors::new(opts.color);
    let mut out = String::new();
    if let Some(h) = header {
        out.push_str(&colors.header(&h));
        out.push_str(end);
    }
    for (path, (x, y, from)) in &rows {
        let unmerged_row = unmerged.contains_key(path);
        let xy = colors.columns(*x, *y, unmerged_row);
        match from {
            Some(from) if opts.nul => out.push_str(&format!("{}{} {}\0{}\0", x, y, path, from)),
            Some(from) => out.push_str(&format!("{} {} -> {}\n", xy, show(from), show(path))),
            None => out.push_str(&format!("{} {}{}", xy, show(path), end)),
        }
    }
    for path in untracked {
        out.push_str(&format!("{} {}{}", colors.paint("??", &colors.untracked), show(path), end));
    }
    out
}

/// git's `color.status.*` colors for the short format (all empty when not coloring).
// Used by `format_short_status`.
struct ShortColors {
    on: bool,
    updated: String,
    changed: String,
    untracked: String,
    unmerged: String,
    local: String,
    remote: String,
}

impl ShortColors {
    fn new(on: bool) -> ShortColors {
        let get = |name: &str, default: &'static str| if on { crate::core::color::slot("status", name, default) } else { String::new() };
        ShortColors {
            on,
            updated: get("added", crate::core::color::GREEN),
            changed: get("changed", crate::core::color::RED),
            untracked: get("untracked", crate::core::color::RED),
            unmerged: get("unmerged", crate::core::color::RED),
            local: get("localbranch", crate::core::color::GREEN),
            remote: get("remotebranch", crate::core::color::RED),
        }
    }

    fn paint(&self, text: &str, color: &str) -> String {
        crate::core::color::paint(text, color)
    }

    /// `XY`: the index column in the updated color, the work-tree column in the changed
    /// color, both in the unmerged color for a conflict (spaces stay uncolored).
    fn columns(&self, x: char, y: char, unmerged: bool) -> String {
        let one = |c: char, color: &str| if c == ' ' { " ".to_string() } else { self.paint(&c.to_string(), color) };
        if unmerged {
            return format!("{}{}", one(x, &self.unmerged), one(y, &self.unmerged));
        }
        format!("{}{}", one(x, &self.updated), one(y, &self.changed))
    }

    /// `## main...origin/main [ahead 1, behind 2]`: the branch green, the upstream red, the
    /// ahead count green and the behind count red, as git colors it.
    fn header(&self, header: &str) -> String {
        if !self.on {
            return header.to_string();
        }
        let Some(rest) = header.strip_prefix("## ") else { return header.to_string() };
        if rest.starts_with("HEAD (no branch)") || rest.starts_with("No commits yet on ") {
            let (label, branch) = match rest.strip_prefix("No commits yet on ") {
                Some(branch) => ("No commits yet on ", branch),
                None => ("", rest),
            };
            let color = if label.is_empty() { crate::core::color::slot("status", "nobranch", crate::core::color::RED) } else { self.local.clone() };
            return format!("## {}{}", label, self.paint(branch, &color));
        }
        let (names, track) = match rest.find(" [") {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let names = match names.split_once("...") {
            Some((local, upstream)) => format!("{}...{}", self.paint(local, &self.local), self.paint(upstream, &self.remote)),
            None => self.paint(names, &self.local),
        };
        let track = color_counts(track, &self.local, &self.remote);
        format!("## {}{}", names, track)
    }
}

/// ` [ahead 3, behind 2]` with the ahead count in `ahead` and the behind count in `behind`.
// Used by `ShortColors::header`.
fn color_counts(track: &str, ahead: &str, behind: &str) -> String {
    let mut out = String::new();
    let mut rest = track;
    while let Some(i) = rest.find(|c: char| c.is_ascii_digit()) {
        let end = rest[i..].find(|c: char| !c.is_ascii_digit()).map(|e| i + e).unwrap_or(rest.len());
        let color = if rest[..i].ends_with("behind ") { behind } else { ahead };
        out.push_str(&rest[..i]);
        out.push_str(&crate::core::color::paint(&rest[i..end], color));
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

