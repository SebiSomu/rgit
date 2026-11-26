//! Finding lost work for `rgit recover`: commits that no ref, HEAD, the index, the stash list
//! or an operation in progress keeps, found through every reflog and every object in the
//! repository, reduced to the newest commit of each lost line.

use std::collections::{HashMap, HashSet};

use anyhow::Result;

use crate::core::object_store::read_object;
use crate::core::reflog::{list_reflog_refs, read_reflog, NULL_HASH};
use crate::workspace::recover::types::{CommitFacts, LostKind, LostWork};

/// Every piece of lost work in the repository, newest first (ties by name, so the numbers
/// `rgit recover` shows stay the same between runs).
// Used by `rgit recover`.
pub fn find_lost_work() -> Result<Vec<LostWork>> {
    let mut facts: HashMap<String, Option<CommitFacts>> = HashMap::new();
    let mut lookup = |hash: &str| -> Option<CommitFacts> { facts.entry(hash.to_string()).or_insert_with(|| commit_facts(hash)).clone() };

    let kept = kept_commits(&mut lookup)?;

    // Candidates: what reflogs remember, each with the reflog line that tells how it got lost
    // (the newest entry that moved away from it, else the newest that names it), then every
    // commit object in the repository.
    let mut reflog_entry: HashMap<String, (bool, String)> = HashMap::new();
    let mut candidates: Vec<String> = Vec::new();
    for ref_name in list_reflog_refs()? {
        let shown = ref_name.strip_prefix("refs/heads/").unwrap_or(&ref_name).to_string();
        // Newest first, so `n` is `<ref>@{n}`.
        for (n, entry) in read_reflog(&ref_name).unwrap_or_default().iter().enumerate() {
            for (hash, moved_away) in [(&entry.old_hash, true), (&entry.new_hash, false)] {
                if hash == NULL_HASH || kept.contains(hash) || lookup(hash).is_none() {
                    continue;
                }
                let line = format!("{}@{{{}}}: {}", shown, n, entry.message);
                match reflog_entry.get(hash) {
                    None => {
                        candidates.push(hash.clone());
                        reflog_entry.insert(hash.clone(), (moved_away, line));
                    }
                    Some((false, _)) if moved_away => {
                        reflog_entry.insert(hash.clone(), (true, line));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    for hash in all_commit_objects()? {
        if !kept.contains(&hash) && !reflog_entry.contains_key(&hash) {
            candidates.push(hash);
        }
    }

    let tips = lost_tips(&candidates, &kept, &mut lookup);
    let mut lost: Vec<LostWork> = tips
        .into_iter()
        .filter_map(|(commit, commits)| {
            let info = lookup(&commit)?;
            let kind = if is_stash_commit(&info, &mut lookup) {
                LostKind::Stash
            } else if is_stash_part(&info.subject) {
                return None;
            } else if let Some((_, entry)) = reflog_entry.get(&commit) {
                LostKind::LeftBehind { entry: entry.clone() }
            } else {
                LostKind::Dangling
            };
            Some(LostWork { commit, kind, subject: info.subject, time: info.time, tz: info.tz, commits })
        })
        .collect();
    lost.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.commit.cmp(&b.commit)));
    Ok(lost)
}

/// The lost commits worth showing: of the `candidates` (commits not in `kept`), those that
/// no other candidate reaches, each with the number of lost commits behind it (itself
/// included). A rebase or reset that left five commits behind shows up once, as its tip.
// Used by `find_lost_work` (and its unit tests).
pub fn lost_tips(candidates: &[String], kept: &HashSet<String>, lookup: &mut dyn FnMut(&str) -> Option<CommitFacts>) -> Vec<(String, usize)> {
    let candidate_set: HashSet<&String> = candidates.iter().collect();
    let mut covered: HashSet<String> = HashSet::new();
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut seen_candidates: HashSet<&String> = HashSet::new();
    for tip in candidates {
        // A candidate another one already reaches can't be a tip, and what it reaches is
        // covered by that one too.
        if !seen_candidates.insert(tip) || covered.contains(tip) {
            continue;
        }
        // The lost history behind this candidate.
        let mut behind: HashSet<String> = HashSet::new();
        let mut stack = vec![tip.clone()];
        while let Some(hash) = stack.pop() {
            if kept.contains(&hash) || !behind.insert(hash.clone()) {
                continue;
            }
            if let Some(facts) = lookup(&hash) {
                stack.extend(facts.parents);
            }
        }
        for hash in &behind {
            if hash != tip && candidate_set.contains(hash) {
                covered.insert(hash.clone());
            }
        }
        counts.push((tip.clone(), behind.len()));
    }
    counts.into_iter().filter(|(tip, _)| !covered.contains(tip)).collect()
}

/// Commits that are not lost: everything reachable from the refs (tags peeled), HEAD of every
/// worktree, the index, the stash list and the state of operations in progress, as `gc`
/// keeps them. Reflogs and `ORIG_HEAD` don't count: what only they remember is lost work.
// Used by `find_lost_work`.
fn kept_commits(lookup: &mut dyn FnMut(&str) -> Option<CommitFacts>) -> Result<HashSet<String>> {
    let roots = crate::storage::fsck::engine::collect_labeled_roots(false)?;
    let mut kept: HashSet<String> = HashSet::new();
    // Index entries are blobs: they keep no commit.
    let mut stack: Vec<String> = roots
        .into_iter()
        .filter(|(_, label, strict)| *strict && !label.contains("index entry"))
        .map(|(hash, _, _)| hash)
        .collect();
    while let Some(hash) = stack.pop() {
        if kept.contains(&hash) {
            continue;
        }
        if let Some(facts) = lookup(&hash) {
            kept.insert(hash);
            stack.extend(facts.parents);
        } else if let Some(target) = tag_target(&hash) {
            stack.push(target);
        }
    }
    Ok(kept)
}

/// The object an annotated tag points at.
// Used by `kept_commits`.
fn tag_target(hash: &str) -> Option<String> {
    let (kind, content) = read_object(hash).ok()?;
    if kind != "tag" {
        return None;
    }
    String::from_utf8_lossy(&content).lines().next()?.strip_prefix("object ").map(|s| s.trim().to_string())
}

/// Every commit object in the repository, loose or packed.
// Used by `find_lost_work`.
fn all_commit_objects() -> Result<Vec<String>> {
    let mut out = Vec::new();
    for object in crate::core::object_store::list_loose_objects()? {
        if read_object(&object.hash).is_ok_and(|(kind, _)| kind == "commit") {
            out.push(object.hash);
        }
    }
    for pack in crate::core::pack::repo_packs().iter() {
        let bytes = crate::core::pack::pack_bytes(pack);
        for (hash, offset) in pack.index.hashes.iter().zip(&pack.index.offsets) {
            let Ok(entry) = crate::core::pack::parse_entry_at(bytes, *offset) else { continue };
            let hash = hex::encode(hash);
            // Commits are almost never deltified; a delta's type is known only once resolved.
            let is_commit = match entry.type_code {
                1 => true,
                6 | 7 => read_object(&hash).is_ok_and(|(kind, _)| kind == "commit"),
                _ => false,
            };
            if is_commit {
                out.push(hash);
            }
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Parents, committer time and zone, and subject of a commit; `None` for a missing object or
/// one that isn't a commit.
// Used by `find_lost_work`.
pub fn commit_facts(hash: &str) -> Option<CommitFacts> {
    let (kind, content) = read_object(hash).ok()?;
    if kind != "commit" {
        return None;
    }
    let text = String::from_utf8_lossy(&content);
    let (headers, message) = text.split_once("\n\n").unwrap_or((&text, ""));
    let mut facts = CommitFacts { subject: message.lines().next().unwrap_or("").trim().to_string(), ..Default::default() };
    for line in headers.lines() {
        if let Some(parent) = line.strip_prefix("parent ") {
            facts.parents.push(parent.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("committer ") {
            let mut fields = rest.rsplitn(3, ' ');
            facts.tz = fields.next().unwrap_or("+0000").to_string();
            facts.time = fields.next().and_then(|t| t.parse().ok()).unwrap_or(0);
        }
    }
    Some(facts)
}

/// Whether a commit has a stash's shape: `WIP on <branch>: ...` or `On <branch>: ...`, with
/// the index saved as its second parent (`index on ...`). `rgit wip` saves look the same.
// Used by `find_lost_work`.
fn is_stash_commit(facts: &CommitFacts, lookup: &mut dyn FnMut(&str) -> Option<CommitFacts>) -> bool {
    let named = facts.subject.starts_with("WIP on ") || (facts.subject.starts_with("On ") && facts.subject.contains(": "));
    named && facts.parents.get(1).and_then(|p| lookup(p)).is_some_and(|index| index.subject.starts_with("index on "))
}

/// The commits a stash is made of besides its top one (the saved index and untracked
/// files): never worth listing on their own.
// Used by `find_lost_work`.
fn is_stash_part(subject: &str) -> bool {
    subject.starts_with("index on ") || subject.starts_with("untracked files on ")
}

/// `2026-11-26 14:03` in the commit's own timezone.
// Used by `rgit recover` (the listing).
pub fn format_when(time: i64, tz: &str) -> String {
    let local = time + crate::core::date::tz_offset_seconds(tz);
    let (year, month, day) = crate::core::date::civil_from_days(local.div_euclid(86_400));
    let secs = local.rem_euclid(86_400);
    format!("{:04}-{:02}-{:02} {:02}:{:02}", year, month, day, secs / 3600, secs % 3600 / 60)
}

#[cfg(test)]
#[path = "../../../tests/unit/workspace/recover.rs"]
mod tests;
