use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use crate::commands::plumbing::write_tree_from_index_prefix;
use crate::helpers::{build_merge_commit, check_switch_safety, find_merge_base, generate_conflict_markers, is_reachable, read_object, resolve_commit_from_source, resolve_tree_from_source, sync_working_tree, update_index_from_tree};
use crate::index::{build_entry, read_index, write_index};
use crate::objects::MergeConflict;
use crate::refs;

pub fn merge(branch: String) -> anyhow::Result<()> {
    let head_state = refs::resolve_head()?;
    let current_branch = match head_state {
        refs::HeadState::Branch(b) => b,
        refs::HeadState::Detached(_) => {
            anyhow::bail!("fatal: You are in 'detached HEAD' state. Please switch to a branch before merging.");
        }
    };

    let our_commit = refs::resolve_head_commit()?.ok_or_else(|| {
        anyhow::anyhow!("fatal: HEAD has no commits yet")
    })?;

    let their_commit = resolve_commit_from_source(&branch)?;

    if our_commit == their_commit {
        println!("Already up to date.");
        return Ok(());
    }

    if is_reachable(&our_commit, &their_commit)? {
        println!("Already up to date.");
        return Ok(());
    }

    if is_reachable(&their_commit, &our_commit)? {
        let target_tree = resolve_tree_from_source(&their_commit)?;
        let head_tree = resolve_tree_from_source(&our_commit)?;
        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

        check_switch_safety(&target_tree, &head_tree, &index_map)?;
        sync_working_tree(&target_tree, &head_tree, &index_map)?;
        update_index_from_tree(&target_tree, &head_tree)?;

        let branch_ref = format!("refs/heads/{}", current_branch);
        refs::write_ref(&branch_ref, &their_commit)?;

        println!("Updating {}..{}", &our_commit[..7], &their_commit[..7]);
        println!("Fast-forward");
        return Ok(());
    }

    let base_commit = find_merge_base(&our_commit, &their_commit)?.ok_or_else(|| {
        anyhow::anyhow!("fatal: refusing to merge unrelated histories")
    })?;

    let base_tree = resolve_tree_from_source(&base_commit)?;
    let our_tree = resolve_tree_from_source(&our_commit)?;
    let their_tree = resolve_tree_from_source(&their_commit)?;

    let mut all_paths = std::collections::BTreeSet::new();
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
        } else if our_entry == base_entry {
            if let Some(entry) = their_entry {
                merged_tree.insert(path, *entry);
            }
        } else if their_entry == base_entry {
            if let Some(entry) = our_entry {
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

    if !conflicts.is_empty() {
        for conflict in &conflicts {
            let conflict_content = generate_conflict_markers(&conflict.ours, &conflict.theirs, &branch);
            if let Some(parent) = Path::new(&conflict.path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(&conflict.path, &conflict_content)?;
            println!("CONFLICT (content): Merge conflict in {}", conflict.path);
        }

        for (path, (hash, _mode)) in &merged_tree {
            let (_, content) = read_object(&hex::encode(hash))?;
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            fs::write(path, &content)?;
        }

        fs::write(".git/MERGE_HEAD", format!("{}\n", their_commit))?;
        fs::write(".git/MERGE_MSG", format!("Merge branch '{}'\n", branch))?;

        println!("Automatic merge failed; fix conflicts and then commit the result.");
        return Ok(());
    }

    for path in our_tree.keys() {
        if !merged_tree.contains_key(path) {
            let path_obj = Path::new(path);
            if path_obj.exists() {
                let _ = fs::remove_file(path_obj);
            }
        }
    }

    let mut new_index_entries = Vec::new();
    for (path, (hash, _mode)) in &merged_tree {
        let (_, content) = read_object(&hex::encode(hash))?;
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, &content)?;

        let metadata = fs::metadata(path)?;
        let entry = build_entry(path, *hash, &metadata);
        new_index_entries.push(entry);
    }

    write_index(&mut new_index_entries)?;

    let tree_hash = write_tree_from_index_prefix(&new_index_entries, "")?;
    let merge_msg = format!("Merge branch '{}'", branch);
    let commit_hash = build_merge_commit(tree_hash, &our_commit, &their_commit, &merge_msg)?;
    let short_hash = &commit_hash[..7];

    let branch_ref = format!("refs/heads/{}", current_branch);
    refs::write_ref(&branch_ref, &commit_hash)?;

    println!("Merge made by the 'ort' strategy.");
    println!("[{} {}] {}", current_branch, short_hash, merge_msg);

    Ok(())
}