use std::collections::{BTreeMap, BTreeSet};
use crate::helpers::objects::{read_object, write_object};
use crate::objects::MergeConflict;

/// Runs a git-style three-way tree merge given base/ours/theirs, returning the
/// resulting merged tree plus any paths that conflict and need manual resolution.
/// Identical to the diff loop `merge` uses inline, factored out here so both
/// `cherry_pick` and `cherry_pick_abort` (which has to recompute what an in-progress pick touched) can share it.
// Used by `cherry_pick` and `cherry_pick_abort`.
pub fn three_way_tree_merge(base_tree: &BTreeMap<String, ([u8; 20], u32)>, our_tree: &BTreeMap<String, ([u8; 20], u32)>, their_tree: &BTreeMap<String, ([u8; 20], u32)>) -> anyhow::Result<(BTreeMap<String, ([u8; 20], u32)>, Vec<MergeConflict>)> {
    let mut all_paths = BTreeSet::new();
    for p in base_tree.keys() { all_paths.insert(p.clone()); }
    for p in our_tree.keys() { all_paths.insert(p.clone()); }
    for p in their_tree.keys() { all_paths.insert(p.clone()); }

    let mut merged_tree: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
    let mut conflicts: Vec<MergeConflict> = Vec::new();

    for path in all_paths {
        let base_entry = base_tree.get(&path);
        let our_entry = our_tree.get(&path);
        let their_entry = their_tree.get(&path);

        if our_entry == their_entry {
            if let Some(entry) = our_entry {
                merged_tree.insert(path, *entry);
            }
        } else if their_entry == base_entry {
            if let Some(entry) = our_entry {
                merged_tree.insert(path, *entry);
            }
        } else if our_entry == base_entry {
            if let Some(entry) = their_entry {
                merged_tree.insert(path, *entry);
            }
        } else {
            let our_bytes = if let Some(our) = our_entry {
                read_object(&hex::encode(our.0))?.1
            } else {
                Vec::new()
            };
            let their_bytes = if let Some(their) = their_entry {
                read_object(&hex::encode(their.0))?.1
            } else {
                Vec::new()
            };
            let base_bytes = if let Some(base) = base_entry {
                read_object(&hex::encode(base.0))?.1
            } else {
                Vec::new()
            };

            conflicts.push(MergeConflict {
                path,
                base: base_bytes,
                ours: our_bytes,
                theirs: their_bytes,
            });
        }
    }

    Ok((merged_tree, conflicts))
}

/// Generates file content containing conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`).
// Used by the `merge` command when conflicts are detected.
pub fn generate_conflict_markers(ours_content: &[u8], theirs_content: &[u8], branch_name: &str) -> Vec<u8> {
    let ours_str = String::from_utf8_lossy(ours_content);
    let theirs_str = String::from_utf8_lossy(theirs_content);

    let mut out = String::new();
    out.push_str("<<<<<<< HEAD\n");
    out.push_str(&ours_str);
    if !ours_str.ends_with('\n') && !ours_str.is_empty() {
        out.push('\n');
    }
    out.push_str("=======\n");
    out.push_str(&theirs_str);
    if !theirs_str.ends_with('\n') && !theirs_str.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!(">>>>>>> {}\n", branch_name));

    out.into_bytes()
}

/// Finds the best common ancestor (merge base) between two commits using BFS.
// Used by the `merge` command for 3-way merges.
pub fn find_merge_base(commit_a: &str, commit_b: &str) -> anyhow::Result<Option<String>> {
    use std::collections::{HashSet, VecDeque};

    if commit_a == commit_b {
        return Ok(Some(commit_a.to_string()));
    }

    let mut ancestors_a = HashSet::new();
    let mut queue_a = VecDeque::new();
    queue_a.push_back(commit_a.to_string());

    while let Some(hash) = queue_a.pop_front() {
        if ancestors_a.contains(&hash) {
            continue;
        }
        ancestors_a.insert(hash.clone());

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

    while let Some(hash) = queue_b.pop_front() {
        if visited_b.contains(&hash) {
            continue;
        }
        visited_b.insert(hash.clone());

        if ancestors_a.contains(&hash) {
            return Ok(Some(hash));
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

    Ok(None)
}

/// Builds and writes a merge commit object containing two parent hashes.
// Used by the `merge` command when creating a 3-way merge commit.
pub fn build_merge_commit(tree_hash: String, parent1: &str, parent2: &str, message: &str) -> anyhow::Result<String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let mut content = format!("tree {}\n", tree_hash);
    content.push_str(&format!("parent {}\n", parent1));
    content.push_str(&format!("parent {}\n", parent2));

    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let author = "rgit <rgit@example.com>";

    content.push_str(&format!("author {} {} +0000\n", author, timestamp));
    content.push_str(&format!("committer {} {} +0000\n", author, timestamp));
    content.push_str(&format!("\n{}\n", message));

    write_object("commit", content.as_bytes())
}