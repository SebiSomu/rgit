use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use crate::helpers::bisect::collect_ancestors;
use crate::helpers::commit::current_signature;
use crate::helpers::format_patch::parse_commit_object;
use crate::helpers::rev_list::list_all_refs;
use crate::helpers::rev_parse::peel_to_type;
use crate::helpers::{read_object, write_object};

/// Lists every tag (loose and packed) as `name -> object` (a tag object for annotated tags,
/// otherwise what the tag points at), in name order.
// Used by `tag` and `describe`.
pub fn list_tag_refs() -> BTreeMap<String, String> {
    list_all_refs()
        .into_iter()
        .filter_map(|(name, hash)| name.strip_prefix("refs/tags/").map(|n| (n.to_string(), hash)))
        .collect()
}

/// Writes an annotated tag object naming `target` (whose type is recorded) with the current
/// identity as tagger, and returns its hash.
// Used by `tag -a` / `tag -m` / `tag -F`.
pub fn write_tag_object(name: &str, target: &str, message: &str) -> anyhow::Result<String> {
    let (kind, _) = read_object(target)?;
    let tagger = current_signature()?;
    let content = format!(
        "object {}\ntype {}\ntag {}\ntagger {} <{}> {} {}\n\n{}",
        target, kind, name, tagger.name, tagger.email, tagger.timestamp, tagger.tz, message
    );
    write_object("tag", content.as_bytes())
}

/// The message lines `tag -n` shows for a tag: an annotated tag's own message, or the message
/// of the commit a lightweight tag points at (nothing for other objects).
// Used by `tag -n`.
pub fn tag_message_lines(object: &str) -> Vec<String> {
    let Ok((kind, content)) = read_object(object) else { return Vec::new() };
    let text = String::from_utf8_lossy(&content).into_owned();
    let message = match kind.as_str() {
        "tag" | "commit" => text.split_once("\n\n").map(|(_, m)| m.to_string()).unwrap_or_default(),
        _ => String::new(),
    };
    let message = message.split("-----BEGIN PGP SIGNATURE-----").next().unwrap_or("").trim_end();
    message.lines().map(str::to_string).collect()
}

/// Deletes a tag ref, whether it is a loose file, a `packed-refs` entry, or both (removing
/// the peeled `^` line that follows a packed annotated tag).
// Used by `tag -d`.
pub fn delete_tag_ref(name: &str) -> anyhow::Result<()> {
    let refname = format!("refs/tags/{}", name);
    let loose = crate::repo::path(format!(".git/{}", refname));
    if loose.exists() {
        fs::remove_file(&loose)?;
        let tags_root = crate::repo::path(".git/refs/tags");
        let mut dir = loose.parent();
        while let Some(d) = dir {
            if d == tags_root || fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    let packed_path = crate::repo::path(".git/packed-refs");
    if let Ok(packed) = fs::read_to_string(&packed_path) {
        let mut kept = Vec::new();
        let mut skipping = false;
        for line in packed.lines() {
            if skipping && line.starts_with('^') {
                continue;
            }
            skipping = line.split_once(' ').is_some_and(|(_, n)| n == refname);
            if !skipping {
                kept.push(line);
            }
        }
        let rewritten = format!("{}\n", kept.join("\n"));
        if rewritten != packed {
            fs::write(&packed_path, rewritten)?;
        }
    }
    Ok(())
}

/// Finds the tag to describe `target` with, as git's `describe` does: candidate tags are
/// annotated ones (plus lightweight ones with `all_tags`), each on the commit it peels to,
/// preferring an annotated tag (then the newest one) when several share a commit. A tag on
/// `target` itself is an exact match (depth 0); otherwise the candidate with the fewest commits
/// reachable from `target` but not from it wins, ties going to the more recent commit.
/// Returns `(tag name, depth)`, or `None` when no candidate is an ancestor.
// Used by `describe`.
pub fn describe_commit(target: &str, all_tags: bool) -> anyhow::Result<Option<(String, usize)>> {
    let mut by_commit: HashMap<String, (String, bool, i64)> = HashMap::new();
    for (name, object) in list_tag_refs() {
        let Ok((kind, content)) = read_object(&object) else { continue };
        let annotated = kind == "tag";
        if !annotated && !all_tags {
            continue;
        }
        let Ok(commit) = peel_to_type(object.clone(), "commit") else { continue };
        let tagger_time = if annotated {
            String::from_utf8_lossy(&content)
                .lines()
                .find_map(|l| l.strip_prefix("tagger "))
                .and_then(|t| t.rsplit(' ').nth(1).and_then(|ts| ts.parse().ok()))
                .unwrap_or(0)
        } else {
            0
        };
        let better = match by_commit.get(&commit) {
            None => true,
            Some((_, was_annotated, was_time)) => (annotated && !was_annotated) || (annotated == *was_annotated && annotated && tagger_time > *was_time),
        };
        if better {
            by_commit.insert(commit, (name, annotated, tagger_time));
        }
    }
    if let Some((name, _, _)) = by_commit.get(target) {
        return Ok(Some((name.clone(), 0)));
    }

    let reachable: HashSet<String> = collect_ancestors(target)?;
    let mut best: Option<(usize, i64, String)> = None;
    for (commit, (name, _, _)) in &by_commit {
        if !reachable.contains(commit) {
            continue;
        }
        let covered = collect_ancestors(commit)?;
        let depth = reachable.iter().filter(|c| !covered.contains(*c)).count();
        let date = parse_commit_object(commit).map(|c| c.committer.timestamp).unwrap_or(0);
        let wins = match &best {
            None => true,
            Some((d, t, _)) => depth < *d || (depth == *d && date > *t),
        };
        if wins {
            best = Some((depth, date, name.clone()));
        }
    }
    Ok(best.map(|(depth, _, name)| (name, depth)))
}
