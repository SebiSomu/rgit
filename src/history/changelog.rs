pub mod cli;
pub mod engine;
pub mod types;

use crate::history::changelog::engine::{entries, latest_tag, render_section, tags_by_date};

/// `rgit changelog [<a>..<b>] [--version <name>] [--all] [--prepend <file>]`: a Markdown
/// changelog from conventional-commit messages (`feat(parser): ...`, `fix!: ...`): a section
/// per version with the breaking changes first, then the commits grouped by type (features,
/// bug fixes, performance, ..., other changes) and the contributors. Default range: since the
/// latest tag HEAD has (`Unreleased`), or all of history; `--all` makes one section per tag.
/// Merges are left out; the date is the newest commit's (author time). `--prepend` adds the
/// result at the top of a file (below its `# ` title) instead of printing it.
pub fn changelog(range: Option<String>, version: Option<String>, all: bool, prepend: Option<String>) -> anyhow::Result<()> {
    let resolve = |rev: &str| match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", if rev.is_empty() { "HEAD" } else { rev })) {
        Ok(hash) => hash,
        Err(_) => crate::ui::die_fatal(format!("'{}' is not a branch, tag or commit", rev)),
    };
    let Some(head) = crate::core::refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits yet");
    };
    let day = |ts: Option<i64>| ts.map(crate::history::story::engine::day);
    let mut text = String::new();
    if all {
        let tags: Vec<(String, String, i64)> = tags_by_date()?.into_iter().filter(|(_, h, _)| *h == head || crate::core::commit::is_reachable(&head, h).unwrap_or(false)).collect();
        let mut sections = Vec::new();
        let mut from: Option<String> = None;
        for (tag, hash, _) in &tags {
            let (list, newest) = entries(hash, from.as_deref())?;
            sections.push(render_section(tag, day(newest), &list));
            from = Some(hash.clone());
        }
        if tags.last().is_none_or(|(_, h, _)| *h != head) {
            let (list, newest) = entries(&head, from.as_deref())?;
            if !list.is_empty() {
                sections.push(render_section(version.as_deref().unwrap_or("Unreleased"), day(newest), &list));
            }
        }
        sections.reverse();
        text = sections.join("\n");
    } else {
        let (from, to, to_name) = match range.as_deref() {
            Some(r) => match r.split_once("..") {
                Some((a, b)) => (Some(resolve(a)), resolve(b), (!b.is_empty()).then(|| b.to_string())),
                None => (None, resolve(r), Some(r.to_string())),
            },
            // HEAD tagged: that tag's section (since the tag before it); else what came since the
            // latest tag.
            None => match latest_tag(&head, false)? {
                Some((tag, hash)) if hash == head => (latest_tag(&head, true)?.map(|(_, h)| h), head.clone(), Some(tag)),
                other => (other.map(|(_, h)| h), head.clone(), None),
            },
        };
        let (list, newest) = entries(&to, from.as_deref())?;
        let tag_here = crate::core::refs::list_tags()?.into_iter().find(|t| to_name.as_deref() == Some(t.as_str()));
        let name = version.or(tag_here).unwrap_or_else(|| "Unreleased".to_string());
        text.push_str(&render_section(&name, day(newest), &list));
    }

    match prepend {
        None => print!("{}", text),
        Some(file) => {
            let existing = std::fs::read_to_string(&file).unwrap_or_default();
            let heading = text.lines().next().unwrap_or("").to_string();
            if existing.lines().any(|l| l == heading) {
                crate::ui::die_fatal(format!("{} already has a section '{}'; nothing written", file, heading.trim_start_matches("## ")));
            }
            let updated = match existing.split_once('\n').filter(|(first, _)| first.starts_with("# ")) {
                Some((title, rest)) => format!("{}\n\n{}\n{}", title, text, rest.trim_start_matches('\n')),
                None if existing.is_empty() => format!("# Changelog\n\n{}", text),
                None => format!("{}\n{}", text, existing),
            };
            std::fs::write(&file, updated)?;
            println!("Added '{}' to {}.", heading.trim_start_matches("## "), file);
        }
    }
    Ok(())
}
