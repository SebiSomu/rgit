use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;
use anyhow::{Context, Result};

use crate::helpers::checkout::{check_switch_safety, sync_working_tree, update_index_from_tree};
use crate::helpers::commit::{commit_subject_line, is_reachable, resolve_commit_from_source};
use crate::helpers::objects::{flatten_tree, read_object, tree_hash_of_commit};
use crate::helpers::state_files::{append_line, read_lines_set};
use crate::index::read_index;
use crate::objects::BisectOutcome;
use crate::refs;

/// Collects the full ancestor set of `start` (inclusive), following parent
/// links via BFS.
pub fn collect_ancestors(start: &str) -> Result<HashSet<String>> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(start.to_string());

    while let Some(hash) = queue.pop_front() {
        if visited.contains(&hash) {
            continue;
        }
        visited.insert(hash.clone());

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }
        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent) = line.strip_prefix("parent ") {
                queue.push_back(parent.to_string());
            }
        }
    }

    Ok(visited)
}

/// BFS from `start` over the *entire* ancestor graph (so it can walk through
/// non-candidate commits to reach further candidates), collecting only the
/// commits that are members of `candidates`, in BFS discovery order.
pub fn order_candidates_from(start: &str, candidates: &HashSet<String>) -> Result<Vec<String>> {
    let mut order = Vec::new();
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(start.to_string());

    while let Some(hash) = queue.pop_front() {
        if visited.contains(&hash) {
            continue;
        }
        visited.insert(hash.clone());

        if candidates.contains(&hash) {
            order.push(hash.clone());
        }

        let (object_type, content) = read_object(&hash)?;
        if object_type != "commit" {
            continue;
        }
        let text = String::from_utf8_lossy(&content);
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(parent) = line.strip_prefix("parent ") {
                queue.push_back(parent.to_string());
            }
        }
    }

    Ok(order)
}

/// Detaches HEAD at `hash` and syncs the working directory + index to match
/// it, refusing (like `switch`) if that would clobber local changes. Shared
/// by every point where bisect moves HEAD: stepping to a new midpoint,
/// `bisect reset <commit>`, and restoring a detached-HEAD starting point.
pub fn checkout_bisect_commit(hash: &str) -> Result<()> {
    let target_tree_hash = tree_hash_of_commit(hash)?;
    let mut target_tree = BTreeMap::new();
    flatten_tree(&target_tree_hash, "", &mut target_tree)?;

    let mut head_tree = BTreeMap::new();
    if let Some(current_commit) = refs::resolve_head_commit()? {
        let current_tree_hash = tree_hash_of_commit(&current_commit)?;
        flatten_tree(&current_tree_hash, "", &mut head_tree)?;
    }

    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

    check_switch_safety(&target_tree, &head_tree, &index_map)?;
    sync_working_tree(&target_tree, &head_tree, &index_map)?;
    update_index_from_tree(&target_tree, &head_tree)?;

    refs::set_head_detached(hash)?;
    Ok(())
}

pub fn print_commit_oneline(hash: &str) -> Result<()> {
    let subject = commit_subject_line(hash)?;
    println!("[{}] {}", &hash[..hash.len().min(7)], subject);
    Ok(())
}

pub fn report_first_bad(hash: &str) -> Result<()> {
    let (object_type, content) = read_object(hash)?;
    let text = String::from_utf8_lossy(&content);

    println!("{} is the first bad commit", hash);
    println!("commit {}", hash);
    if object_type == "commit" {
        for line in text.lines() {
            if line.is_empty() {
                break;
            }
            if let Some(author) = line.strip_prefix("author ") {
                println!("Author: {}", author);
            }
        }
    }
    println!();
    println!("    {}", commit_subject_line(hash)?);
    Ok(())
}

pub fn report_outcome(outcome: &BisectOutcome) -> Result<()> {
    match outcome {
        BisectOutcome::WaitingForBad => {
            println!("status: waiting for bad commit, good commit(s) known");
        }
        BisectOutcome::WaitingForGood => {
            println!("status: waiting for good commit(s), bad commit known");
        }
        BisectOutcome::Continue(hash, remaining) => {
            let steps = if *remaining == 0 { 0 } else { (*remaining as f64).log2().ceil() as u32 };
            println!(
                "Bisecting: {} revision{} left to test after this (roughly {} step{})",
                remaining,
                if *remaining == 1 { "" } else { "s" },
                steps,
                if steps == 1 { "" } else { "s" }
            );
            print_commit_oneline(hash)?;
        }
        BisectOutcome::Found(hash) => {
            report_first_bad(hash)?;
        }
    }
    Ok(())
}

/// Recomputes the candidate set from the current BISECT_BAD/GOOD/SKIP state
/// and either checks out the next midpoint (`Continue`) or concludes the
/// search (`Found`). Pure aside from the checkout side effect on `Continue`.
pub fn bisect_recompute() -> Result<BisectOutcome> {
    let bad = match fs::read_to_string(crate::commands::bisect::BISECT_BAD_PATH)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(b) => b,
        None => return Ok(BisectOutcome::WaitingForBad),
    };

    let goods = read_lines_set(crate::commands::bisect::BISECT_GOOD_PATH)?;
    if goods.is_empty() {
        return Ok(BisectOutcome::WaitingForGood);
    }

    for good in &goods {
        if !is_reachable(&bad, good)? {
            anyhow::bail!(
                "error: some good revs are not ancestors of the bad rev.\n\
                 rgit bisect cannot work properly in this state."
            );
        }
    }

    let ancestors_bad = collect_ancestors(&bad)?;
    let mut ancestors_good: HashSet<String> = HashSet::new();
    for g in &goods {
        ancestors_good.extend(collect_ancestors(g)?);
    }

    let candidates: HashSet<String> = ancestors_bad.difference(&ancestors_good).cloned().collect();

    let skip_set: HashSet<String> = read_lines_set(crate::commands::bisect::BISECT_SKIP_PATH)?.into_iter().collect();
    let testable: HashSet<String> = candidates.difference(&skip_set).cloned().collect();

    if testable.is_empty() {
        if candidates.len() <= 1 {
            let hash = candidates.into_iter().next().unwrap_or_else(|| bad.clone());
            return Ok(BisectOutcome::Found(hash));
        }
        anyhow::bail!(
            "error: every commit left to test has been skipped; cannot narrow down further.\n\
             Try marking a different commit good/bad, or reducing the number of skips."
        );
    }

    let order = order_candidates_from(&bad, &testable)?;

    if order.len() == 1 {
        return Ok(BisectOutcome::Found(order[0].clone()));
    }

    let mid = order[order.len() / 2].clone();
    checkout_bisect_commit(&mid)?;

    let remaining = order.len() - 1;
    Ok(BisectOutcome::Continue(mid, remaining))
}

pub fn mark_bad(rev: Option<String>) -> Result<BisectOutcome> {
    if !Path::new(crate::commands::bisect::BISECT_START_PATH).exists() {
        anyhow::bail!("fatal: You need to start by \"rgit bisect start\"");
    }

    let target = match rev {
        Some(r) => resolve_commit_from_source(&r)?,
        None => refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: bad HEAD - I need a HEAD commit"))?,
    };

    fs::write(crate::commands::bisect::BISECT_BAD_PATH, format!("{}\n", target)).context("Failed to write .git/BISECT_BAD")?;
    append_line(crate::commands::bisect::BISECT_LOG_PATH, &format!("git bisect bad {}", target))?;

    bisect_recompute()
}

pub fn mark_good(rev: Option<String>) -> Result<BisectOutcome> {
    if !Path::new(crate::commands::bisect::BISECT_START_PATH).exists() {
        anyhow::bail!("fatal: You need to start by \"rgit bisect start\"");
    }

    let target = match rev {
        Some(r) => resolve_commit_from_source(&r)?,
        None => refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: bad HEAD - I need a HEAD commit"))?,
    };

    let mut goods = read_lines_set(crate::commands::bisect::BISECT_GOOD_PATH)?;
    if !goods.iter().any(|g| g == &target) {
        goods.push(target.clone());
        let mut content = goods.join("\n");
        content.push('\n');
        fs::write(crate::commands::bisect::BISECT_GOOD_PATH, content).context("Failed to write .git/BISECT_GOOD")?;
    }
    append_line(crate::commands::bisect::BISECT_LOG_PATH, &format!("git bisect good {}", target))?;

    bisect_recompute()
}

pub fn mark_skip(revs: Vec<String>) -> Result<BisectOutcome> {
    if !Path::new(crate::commands::bisect::BISECT_START_PATH).exists() {
        anyhow::bail!("fatal: You need to start by \"rgit bisect start\"");
    }

    let targets: Vec<String> = if revs.is_empty() {
        vec![refs::resolve_head_commit()?
            .ok_or_else(|| anyhow::anyhow!("fatal: bad HEAD - I need a HEAD commit"))?]
    } else {
        revs.iter().map(|r| resolve_commit_from_source(r)).collect::<Result<Vec<_>>>()?
    };

    let mut skips = read_lines_set(crate::commands::bisect::BISECT_SKIP_PATH)?;
    for target in &targets {
        if !skips.iter().any(|s| s == target) {
            skips.push(target.clone());
        }
        append_line(crate::commands::bisect::BISECT_LOG_PATH, &format!("git bisect skip {}", target))?;
    }
    let mut content = skips.join("\n");
    content.push('\n');
    fs::write(crate::commands::bisect::BISECT_SKIP_PATH, content).context("Failed to write .git/BISECT_SKIP")?;

    bisect_recompute()
}