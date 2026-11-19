use std::collections::HashSet;
use crate::core::config::get_config_value;
use crate::helpers::gc::mark_reachable;
use crate::core::object_store::read_object;
use crate::core::refs::list_all_refs;

/// Usage text for `repo`, printed for `-h` and after usage errors (exit 129).
pub const REPO_USAGE: &str = "usage: rgit repo info [--format=(keyvalue|nul) | -z] [--all | <key>...]
   or: rgit repo structure [--format=(table|keyvalue|nul) | -z]
";

/// The keys `repo info` knows, in the order `--all` prints them.
pub const REPO_INFO_KEYS: [&str; 4] = ["layout.bare", "layout.shallow", "object.format", "references.format"];

/// Returns the value of one `repo info` key, or `None` for a key that doesn't exist:
/// `layout.bare` (`core.bare`), `layout.shallow` (whether `.git/shallow` exists),
/// `object.format` (always `sha1`) and `references.format` (always `files`).
// Used by `repo info`.
pub fn repo_info_value(key: &str) -> Option<String> {
    match key {
        "layout.bare" => Some(get_config_value("core.bare").ok().flatten().is_some_and(|v| v.eq_ignore_ascii_case("true")).to_string()),
        "layout.shallow" => Some(crate::core::repo::path(".git/shallow").exists().to_string()),
        "object.format" => Some("sha1".to_string()),
        "references.format" => Some("files".to_string()),
        _ => None,
    }
}

/// Counts what `repo structure` reports, as `(key, count)` pairs: refs by kind (branches,
/// tags, remote-tracking branches, everything else) and the objects reachable from them by
/// type (commits, trees, blobs, annotated tags).
// Used by `repo structure`.
pub fn repo_structure_counts() -> anyhow::Result<Vec<(&'static str, usize)>> {
    let refs = list_all_refs();
    let count_refs = |prefix: &str| refs.keys().filter(|name| name.starts_with(prefix)).count();
    let (branches, tags, remotes) = (count_refs("refs/heads/"), count_refs("refs/tags/"), count_refs("refs/remotes/"));

    let roots: Vec<String> = refs.values().cloned().collect();
    let mut reachable = HashSet::new();
    mark_reachable(&roots, &mut reachable, false)?;
    let (mut commits, mut trees, mut blobs, mut tag_objects) = (0, 0, 0, 0);
    for hash in &reachable {
        match read_object(hash).map(|(kind, _)| kind).as_deref() {
            Ok("commit") => commits += 1,
            Ok("tree") => trees += 1,
            Ok("blob") => blobs += 1,
            Ok("tag") => tag_objects += 1,
            _ => {}
        }
    }
    Ok(vec![
        ("references.branches.count", branches),
        ("references.tags.count", tags),
        ("references.remotes.count", remotes),
        ("references.others.count", refs.len() - branches - tags - remotes),
        ("objects.commits.count", commits),
        ("objects.trees.count", trees),
        ("objects.blobs.count", blobs),
        ("objects.tags.count", tag_objects),
    ])
}

/// Renders the `repo structure` counts as the two-column table git prints by default:
/// a references section and a reachable-objects section, each with its total and one row
/// per kind, with the value column right-aligned.
// Used by `repo structure`.
pub fn format_structure_table(counts: &[(&'static str, usize)]) -> String {
    let get = |key: &str| counts.iter().find(|(k, _)| *k == key).map(|(_, n)| *n).unwrap_or(0);
    let section = |title: &str, rows: &[(&str, &str)]| -> Vec<(String, String)> {
        let total: usize = rows.iter().map(|(_, key)| get(key)).sum();
        let mut out = vec![(format!("* {}", title), String::new()), ("  * Count".to_string(), total.to_string())];
        out.extend(rows.iter().map(|(label, key)| (format!("    * {}", label), get(key).to_string())));
        out
    };
    let mut rows = section(
        "References",
        &[("Branches", "references.branches.count"), ("Tags", "references.tags.count"), ("Remotes", "references.remotes.count"), ("Others", "references.others.count")],
    );
    rows.push((String::new(), String::new()));
    rows.extend(section(
        "Reachable objects",
        &[("Commits", "objects.commits.count"), ("Trees", "objects.trees.count"), ("Blobs", "objects.blobs.count"), ("Tags", "objects.tags.count")],
    ));

    let (name_title, value_title) = ("Repository structure", "Value");
    let name_width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0).max(name_title.len());
    let value_width = rows.iter().map(|(_, value)| value.len()).max().unwrap_or(0).max(value_title.len());
    let mut out = format!("| {:<nw$} | {:<vw$} |\n", name_title, value_title, nw = name_width, vw = value_width);
    out.push_str(&format!("| {} | {} |\n", "-".repeat(name_width), "-".repeat(value_width)));
    for (name, value) in rows {
        out.push_str(&format!("| {:<nw$} | {:>vw$} |\n", name, value, nw = name_width, vw = value_width));
    }
    out
}
