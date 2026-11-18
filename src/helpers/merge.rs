use std::collections::{BTreeMap, BTreeSet};
use crate::helpers::objects::{read_object, write_object};

/// Finds all best common ancestors (merge bases) between two commits.
// Used by `merge_base`.
pub fn find_all_merge_bases(commit_a: &str, commit_b: &str) -> anyhow::Result<Vec<String>> {
    use std::collections::{HashSet, VecDeque};

    if commit_a == commit_b {
        return Ok(vec![commit_a.to_string()]);
    }

    let mut ancestors_a = HashSet::new();
    let mut queue_a = VecDeque::new();
    queue_a.push_back(commit_a.to_string());

    while let Some(hash) = queue_a.pop_front() {
        if !ancestors_a.insert(hash.clone()) {
            continue;
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
            if let Some(parent_hash) = line.strip_prefix("parent ") {
                queue_a.push_back(parent_hash.to_string());
            }
        }
    }

    let mut visited_b = HashSet::new();
    let mut queue_b = VecDeque::new();
    queue_b.push_back(commit_b.to_string());
    let mut candidates = Vec::new();

    while let Some(hash) = queue_b.pop_front() {
        if !visited_b.insert(hash.clone()) {
            continue;
        }

        if ancestors_a.contains(&hash) {
            candidates.push(hash);
            continue;
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
            if let Some(parent_hash) = line.strip_prefix("parent ") {
                queue_b.push_back(parent_hash.to_string());
            }
        }
    }

    let mut merge_bases = Vec::new();
    for (i, c1) in candidates.iter().enumerate() {
        let mut dominated = false;
        for (j, c2) in candidates.iter().enumerate() {
            if i != j && crate::helpers::commit::is_reachable(c2, c1)? {
                dominated = true;
                break;
            }
        }
        if !dominated && !merge_bases.contains(c1) {
            merge_bases.push(c1.clone());
        }
    }

    Ok(merge_bases)
}

/// Finds the best common ancestor (merge base) between two commits using BFS.
// Used by `merge` and `merge_base`.
pub fn find_merge_base(commit_a: &str, commit_b: &str) -> anyhow::Result<Option<String>> {
    let bases = find_all_merge_bases(commit_a, commit_b)?;
    Ok(bases.into_iter().next())
}


/// Builds and writes a merge commit object containing two parent hashes, authored and
/// committed by the configured identity (`user.name` / `user.email`).
// Used by the `merge` command when creating a 3-way merge commit, and by `commit` and `stash`.
pub fn build_merge_commit(tree_hash: String, parent1: &str, parent2: &str, message: &str) -> anyhow::Result<String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut content = format!("tree {}\n", tree_hash);
    content.push_str(&format!("parent {}\n", parent1));
    content.push_str(&format!("parent {}\n", parent2));

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let author = crate::helpers::config::author_identity()?;

    content.push_str(&format!("author {} {} +0000\n", author, timestamp));
    content.push_str(&format!("committer {} {} +0000\n", author, timestamp));
    content.push_str(&format!("\n{}\n", message));

    write_object("commit", content.as_bytes())
}
type TreeMap = BTreeMap<String, ([u8; 20], u32)>;

/// Replaces the index entries of every conflicted path with its unmerged stages: 1 for the
/// common ancestor, 2 for ours and 3 for theirs, leaving out the sides that don't have the
/// path (a modify/delete conflict has no stage 2 or 3; an add/add one no stage 1).
// Used by `merge`, `cherry-pick`, `revert` and `rebase` after a conflicted merge.
pub fn write_conflict_stages(conflicted: &BTreeMap<String, [Option<([u8; 20], u32)>; 3]>) -> anyhow::Result<()> {
    use crate::index::{read_index, write_index, IndexEntry};
    if conflicted.is_empty() {
        return Ok(());
    }
    let mut entries = read_index().unwrap_or_default();
    entries.retain(|e| !conflicted.contains_key(&e.path));
    for (path, sides) in conflicted {
        for (i, side) in sides.iter().enumerate() {
            if let Some((hash, mode)) = side {
                entries.push(IndexEntry::unmerged(path, *hash, *mode, i as u8 + 1));
            }
        }
    }
    write_index(&mut entries)
}

/// Paths that have unmerged (stage 1-3) entries in the index, in index order.
// Used by every command that must refuse to run on an unresolved conflict, and by `status`.
pub fn unmerged_paths(entries: &[crate::index::IndexEntry]) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for entry in entries.iter().filter(|e| e.stage != 0) {
        if paths.last() != Some(&entry.path) {
            paths.push(entry.path.clone());
        }
    }
    paths
}

/// git's `die_resolve_conflict` / `error_resolve_conflict`: when the index has unmerged paths,
/// prints `error: <Verb> is not possible because you have unmerged files.`, the resolve hints,
/// and `fatal: <fatal>`, then exits with 128. Does nothing on a fully merged index.
// Used by `merge`, `cherry-pick`, `revert` and (through `die_if_unmerged_for_commit`) `commit`.
pub fn die_if_unmerged(verb: &str, fatal: &str) -> anyhow::Result<()> {
    resolve_conflict_or_die(verb, fatal, false)
}

/// `commit`'s refusal on an unmerged index: git's `die_if_unmerged("Committing", ...)` text on
/// stderr, then each unmerged path as `U\t<path>` on stdout (the porcelain refresh report,
/// which git flushes last when its output is piped).
// Used by `commit` and the `--continue` of `cherry-pick` and `revert`.
pub fn die_if_unmerged_for_commit() -> anyhow::Result<()> {
    resolve_conflict_or_die("Committing", "Exiting because of an unresolved conflict.", true)
}

fn resolve_conflict_or_die(verb: &str, fatal: &str, list_paths: bool) -> anyhow::Result<()> {
    let entries = crate::index::read_index().unwrap_or_default();
    let paths = unmerged_paths(&entries);
    if paths.is_empty() {
        return Ok(());
    }
    eprintln!("error: {} is not possible because you have unmerged files.", verb);
    eprintln!("hint: Fix them up in the work tree, and then use 'rgit add/rm <file>'");
    eprintln!("hint: as appropriate to mark resolution and make a commit.");
    eprintln!("fatal: {}", fatal);
    if list_paths {
        for path in &paths {
            println!("U\t{}", path);
        }
    }
    std::process::exit(128);
}

/// Prints `<path>: needs merge` for each unmerged path (git's refresh-index report) and
/// returns whether there were any.
// Used by `switch`/`checkout <branch>`, `reset --soft` and the sequencer before they refuse.
pub fn report_needs_merge() -> bool {
    let entries = crate::index::read_index().unwrap_or_default();
    let paths = unmerged_paths(&entries);
    for path in &paths {
        println!("{}: needs merge", path);
    }
    !paths.is_empty()
}

/// Merges `theirs` into `ours` relative to `base` and writes the result into the working tree
/// and index, the way git's sequencer and `merge` do: files that merged cleanly are staged,
/// conflicted files get their marker content in the working tree and their stages 1-3 in the
/// index. Fails without touching anything when local changes would be overwritten (the error
/// names `operation`). Returns the merge result so the caller can print its messages and
/// decide whether to commit.
// Used by `merge`, `cherry-pick`, `revert` and `rebase`.
pub fn merge_into_worktree(base: &TreeMap, ours: &TreeMap, theirs: &TreeMap, our_label: &str, their_label: &str, operation: &str) -> anyhow::Result<crate::objects::TreeMergeResult> {
    merge_into_worktree_favoring(base, ours, theirs, our_label, their_label, operation, crate::objects::MergeFavor::None)
}

/// `merge_into_worktree` that resolves content conflicts in favour of one side (`merge -X`).
// Used by `merge`.
pub fn merge_into_worktree_favoring(base: &TreeMap, ours: &TreeMap, theirs: &TreeMap, our_label: &str, their_label: &str, operation: &str, favor: crate::objects::MergeFavor) -> anyhow::Result<crate::objects::TreeMergeResult> {
    use crate::helpers::checkout::{check_switch_safety, sync_working_tree, update_index_from_tree};
    use crate::helpers::merge_tree::merge_trees_favoring;
    let result = merge_trees_favoring(base, ours, theirs, our_label, their_label, 7, favor)?;
    let index_entries = crate::index::read_index().unwrap_or_default();
    let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().filter(|e| e.stage == 0).map(|e| (e.path.clone(), e.hash)).collect();
    check_switch_safety(&result.tree, ours, &index_map).map_err(|e| {
        anyhow::anyhow!("{}", e.to_string().replace("by switch", &format!("by {}", operation)).replace("before you switch branches", &format!("before you {}", operation)))
    })?;
    sync_working_tree(&result.tree, ours, &index_map)?;
    update_index_from_tree(&result.tree, ours)?;
    write_conflict_stages(&result.conflicted)?;
    Ok(result)
}

/// Removes the files that record a merge in progress or a pending squash: `MERGE_HEAD`,
/// `MERGE_MSG`, `MERGE_MODE`, `SQUASH_MSG` and `AUTO_MERGE`.
// Used by `merge`, `commit` and `reset`.
pub fn clear_merge_state() {
    for name in ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE", "SQUASH_MSG", "AUTO_MERGE"] {
        let _ = std::fs::remove_file(crate::repo::path(format!(".git/{}", name)));
    }
}

/// `merge --abort` (git's `reset --merge`): puts every path the merge touched back to what
/// `HEAD` has, in the index and the working tree, and leaves local changes to other files
/// alone. A path counts as touched when its index entry differs from `HEAD` or is unmerged.
// Used by `merge --abort`.
pub fn abort_merge_to_head() -> anyhow::Result<()> {
    use crate::helpers::checkout::{sync_working_tree, update_index_from_tree};
    let head_tree: TreeMap = match crate::refs::resolve_head_commit()? {
        Some(commit) => crate::helpers::resolve_tree_from_source(&commit)?,
        None => TreeMap::new(),
    };
    let entries = crate::index::read_index().unwrap_or_default();
    // Where the work tree is now (unmerged paths hold marker content: never equal to a blob)
    // and where it should end up (HEAD's version of every touched path).
    let mut current: TreeMap = TreeMap::new();
    let mut target: TreeMap = TreeMap::new();
    for entry in &entries {
        let in_head = head_tree.get(&entry.path).copied();
        if entry.stage != 0 {
            current.insert(entry.path.clone(), ([0u8; 20], 0));
            if let Some(h) = in_head {
                target.insert(entry.path.clone(), h);
            }
        } else if in_head != Some((entry.hash, entry.mode)) {
            current.insert(entry.path.clone(), (entry.hash, entry.mode));
            if let Some(h) = in_head {
                target.insert(entry.path.clone(), h);
            }
        } else {
            current.insert(entry.path.clone(), (entry.hash, entry.mode));
            target.insert(entry.path.clone(), (entry.hash, entry.mode));
        }
    }
    for (path, h) in &head_tree {
        if !current.contains_key(path) {
            // Deleted by the merge: comes back.
            target.insert(path.clone(), *h);
        }
    }
    let index_map: BTreeMap<String, [u8; 20]> = entries.iter().filter(|e| e.stage == 0).map(|e| (e.path.clone(), e.hash)).collect();
    sync_working_tree(&target, &current, &index_map)?;
    update_index_from_tree(&target, &current)?;
    clear_merge_state();
    Ok(())
}
