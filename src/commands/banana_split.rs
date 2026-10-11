use std::collections::{BTreeMap, BTreeSet};
use crate::commands::commit::build_commit;
use crate::commands::plumbing::write_tree_from_index_prefix;
use crate::commands::rebase::{collect_commits_to_replay, rebase_in_progress, replay_onto};
use crate::helpers::checkout::ensure_working_tree_clean;
use crate::helpers::{commit_parents, extract_commit_message, is_reachable, read_object, resolve_commit_from_source, resolve_tree_from_source};
use crate::index::IndexEntry;
use crate::refs;

type Tree = BTreeMap<String, ([u8; 20], u32)>;

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

/// The group a changed path belongs to.
fn group_key(path: &str, by_dir: bool) -> String {
    if by_dir {
        match path.split_once('/') {
            Some((top, _)) => top.to_string(),
            None => ".".to_string(),
        }
    } else {
        path.to_string()
    }
}

/// Writes a tree object for a flattened path -> (hash, mode) map.
fn build_tree_from_map(map: &Tree) -> anyhow::Result<String> {
    let entries: Vec<IndexEntry> = map
        .iter()
        .map(|(path, (hash, mode))| IndexEntry {
            ctime_secs: 0,
            ctime_nsecs: 0,
            mtime_secs: 0,
            mtime_nsecs: 0,
            dev: 0,
            ino: 0,
            mode: *mode,
            uid: 0,
            gid: 0,
            size: 0,
            hash: *hash,
            path: path.clone(),
        })
        .collect();
    write_tree_from_index_prefix(&entries, "")
}

/// `rgit banana-split [--by-dir] [-n] [<commit>]`
pub fn banana_split(commit: Option<String>, by_dir: bool, dry_run: bool) -> anyhow::Result<()> {
    if rebase_in_progress() {
        anyhow::bail!(
            "fatal: a rebase is in progress\n\
             hint: use 'rgit rebase --continue' or 'rgit rebase --abort' first"
        );
    }

    let source = commit.as_deref().unwrap_or("HEAD");
    let target = resolve_commit_from_source(source)?;

    let head_state = refs::resolve_head()?;
    let our_commit = refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;

    if target != our_commit && !is_reachable(&our_commit, &target)? {
        anyhow::bail!(
            "fatal: commit {} is not part of the current branch's history",
            short(&target)
        );
    }

    let (object_type, content) = read_object(&target)?;
    if object_type != "commit" {
        anyhow::bail!("fatal: '{}' does not point to a commit object", source);
    }
    let text = String::from_utf8_lossy(&content).to_string();
    let parents = commit_parents(&text);
    if parents.len() > 1 {
        anyhow::bail!(
            "error: commit {} is a merge commit; splitting merges is not supported",
            short(&target)
        );
    }

    let original_message = extract_commit_message(&text);
    let original_message = original_message.trim_end();
    let (subject, body) = match original_message.split_once('\n') {
        Some((first, rest)) => (first.to_string(), rest.trim().to_string()),
        None => (original_message.to_string(), String::new()),
    };

    let parent_tree: Tree = match parents.first() {
        Some(p) => resolve_tree_from_source(p)?,
        None => BTreeMap::new(),
    };
    let target_tree = resolve_tree_from_source(&target)?;

    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let all_paths: BTreeSet<&String> = parent_tree.keys().chain(target_tree.keys()).collect();
    for path in all_paths {
        if parent_tree.get(path) != target_tree.get(path) {
            groups.entry(group_key(path, by_dir)).or_default().push(path.clone());
        }
    }

    if groups.len() < 2 {
        match groups.keys().next() {
            None => println!("Nothing to split: {} changes no files.", short(&target)),
            Some(only) => println!(
                "Nothing to split: all of {}'s changes are in one {} ({}).",
                short(&target),
                if by_dir { "directory" } else { "file" },
                only
            ),
        }
        return Ok(());
    }

    let n = groups.len();

    if dry_run {
        println!("Would split {} \"{}\" into {} commits:", short(&target), subject, n);
        for (i, (group, paths)) in groups.iter().enumerate() {
            if paths.len() == 1 && paths[0] == *group {
                println!("  {}/{}  {}", i + 1, n, group);
            } else {
                println!("  {}/{}  {}  ({} files)", i + 1, n, group, paths.len());
                for p in paths {
                    println!("         {}", p);
                }
            }
        }
        return Ok(());
    }

    ensure_working_tree_clean("split a commit")?;

    let descendants = if target == our_commit {
        Vec::new()
    } else {
        collect_commits_to_replay(&our_commit, &target)?
    };

    let mut current = parent_tree.clone();
    let mut previous: Option<String> = parents.first().cloned();
    let mut created: Vec<(String, String)> = Vec::new();

    for (i, (group, paths)) in groups.iter().enumerate() {
        for path in paths {
            match target_tree.get(path) {
                Some(entry) => {
                    current.insert(path.clone(), *entry);
                }
                None => {
                    current.remove(path);
                }
            }
        }

        let tree_hash = build_tree_from_map(&current)?;
        let mut message = format!("{} ({}/{}: {})", subject, i + 1, n, group);
        if !body.is_empty() {
            message.push_str(&format!("\n\n{}", body));
        }
        message.push_str(&format!("\n\nSplit from commit {}.", target));

        let hash = build_commit(tree_hash, previous.clone(), &message)?;
        created.push((hash.clone(), group.clone()));
        previous = Some(hash);
    }

    if current != target_tree {
        anyhow::bail!("internal error: the split pieces do not add up to {}; nothing was changed", short(&target));
    }
    let new_tip = previous.expect("at least two groups, so at least one commit was created");

    println!("Split {} \"{}\" into {} commits:", short(&target), subject, n);
    for (hash, group) in &created {
        println!("  [{}] {}", short(hash), group);
    }

    if target == our_commit {
        match &head_state {
            refs::HeadState::Branch(branch_name) => {
                refs::write_ref(&format!("refs/heads/{}", branch_name), &new_tip)?;
            }
            refs::HeadState::Detached(_) => {
                refs::set_head_detached(&new_tip)?;
            }
        }
        return Ok(());
    }

    replay_onto(&head_state, &our_commit, &new_tip, &descendants)
}