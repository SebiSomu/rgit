use std::collections::BTreeMap;
use crate::helpers::{check_switch_safety, flatten_tree, is_reachable, read_object, sync_working_tree, tree_hash_of_commit, update_index_from_tree};
use crate::index::read_index;
use crate::refs;

pub fn branch(name: Option<String>, delete: bool, force_delete: bool, rename: Option<String>) -> anyhow::Result<()> {
    if let Some(new_name) = rename {
        let old_name = match name {
            Some(ref n) => n.as_str().to_string(),
            None => {
                let ref_path = refs::current_branch_ref()?;
                ref_path
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&ref_path)
                    .to_string()
            }
        };
        refs::rename_branch(&old_name, &new_name)?;
        println!("Renamed branch '{}' to '{}'.", old_name, new_name);
        return Ok(());
    }

    if delete || force_delete {
        let branch_name = name.ok_or_else(|| {
            anyhow::anyhow!("fatal: branch name required for delete")
        })?;
        let head = refs::resolve_head()?;
        if let refs::HeadState::Branch(ref current) = head {
            if current == &branch_name {
                anyhow::bail!(
                    "error: Cannot delete branch '{}' checked out locally.",
                    branch_name
                );
            }
        }

        if delete && !force_delete {
            let branch_ref = format!("refs/heads/{}", branch_name);
            if let Some(branch_tip) = refs::read_ref(&branch_ref)? {
                let head_commit = match &head {
                    refs::HeadState::Branch(b) => {
                        let r = format!("refs/heads/{}", b);
                        refs::read_ref(&r)?
                    }
                    refs::HeadState::Detached(h) => Some(h.clone()),
                };

                let merged = if let Some(head_hash) = head_commit {
                    is_reachable(&head_hash, &branch_tip)?
                } else {
                    false
                };

                if !merged {
                    anyhow::bail!("error: The branch '{}' is not fully merged.\n If you are sure you want to delete it, run 'branch -D {}'.", branch_name, branch_name);
                }
            }
        }

        refs::delete_branch(&branch_name)?;
        println!("Deleted branch {}.", branch_name);
        return Ok(());
    }

    match name {
        None => {
            let head_state = refs::resolve_head()?;
            let current_branch = match &head_state {
                refs::HeadState::Branch(b) => Some(b.as_str()),
                refs::HeadState::Detached(_) => None,
            };

            let branches = refs::list_branches()?;

            if branches.is_empty() {
                if let Some(name) = current_branch {
                    println!("* {}", name);
                }
            } else {
                for branch in &branches {
                    if Some(branch.as_str()) == current_branch {
                        println!("* {}", branch);
                    } else {
                        println!("  {}", branch);
                    }
                }
            }
        }
        Some(branch_name) => {
            let ref_path = refs::current_branch_ref()?;
            let commit_hash = refs::read_ref(&ref_path)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "fatal: not a valid object name: '{}' has no commits yet",
                    ref_path.strip_prefix("refs/heads/").unwrap_or(&ref_path)
                )
            })?;
            refs::create_branch(&branch_name, &commit_hash)?;
            println!("Created branch '{}' at {}", branch_name, &commit_hash[..7]);
        }
    }

    Ok(())
}

pub fn switch(branch: String, create: bool, detach: bool, force: bool) -> anyhow::Result<()> {
    if create {
        let commit_hash = refs::resolve_head_commit()?.ok_or_else(|| {
            anyhow::anyhow!("fatal: cannot create branch — no commits yet")
        })?;
        refs::create_branch(&branch, &commit_hash)?;
    }

    let target_commit = if detach {
        let branch_ref = format!("refs/heads/{}", branch);
        if let Some(hash) = refs::read_ref(&branch_ref)? {
            hash
        } else {
            match read_object(&branch) {
                Ok((obj_type, _)) if obj_type == "commit" => branch.clone(),
                Ok((obj_type, _)) => {
                    anyhow::bail!("fatal: '{}' is not a commit (it is a {})", branch, obj_type);
                }
                Err(_) => {
                    anyhow::bail!("fatal: '{}' is not a valid object hash or branch name", branch);
                }
            }
        }
    } else {
        let target_ref = format!("refs/heads/{}", branch);
        match refs::read_ref(&target_ref)? {
            Some(hash) => hash,
            None => anyhow::bail!("fatal: invalid reference: {}", branch),
        }
    };

    if !detach {
        let current_head = refs::resolve_head()?;
        if let refs::HeadState::Branch(ref current_branch) = current_head {
            if current_branch == &branch {
                println!("Already on '{}'", branch);
                return Ok(());
            }
        }
    }

    let target_tree_hash = tree_hash_of_commit(&target_commit)?;
    let mut target_tree = BTreeMap::new();
    flatten_tree(&target_tree_hash, "", &mut target_tree)?;

    let mut head_tree = BTreeMap::new();
    if let Some(current_commit) = refs::resolve_head_commit()? {
        let current_tree_hash = tree_hash_of_commit(&current_commit)?;
        flatten_tree(&current_tree_hash, "", &mut head_tree)?;
    }

    let index_entries = read_index().unwrap_or_default();
    let index_map: BTreeMap<String, [u8; 20]> = index_entries
        .iter()
        .map(|e| (e.path.clone(), e.hash))
        .collect();

    if !force {
        check_switch_safety(&target_tree, &head_tree, &index_map)?;
    }

    sync_working_tree(&target_tree, &head_tree, &index_map)?;
    update_index_from_tree(&target_tree, &head_tree)?;

    if detach {
        refs::set_head_detached(&target_commit)?;
        println!("HEAD is now at {} (detached)", &target_commit[..7]);
        eprintln!("warning: You are in a detached HEAD state.");
        eprintln!("  You can look around, make experimental commits, or switch -c <branch> to keep them.");
    } else if create {
        refs::set_head(&branch)?;
        println!("Switched to a new branch '{}'", branch);
    } else {
        refs::set_head(&branch)?;
        println!("Switched to branch '{}'", branch);
    }

    Ok(())
}

pub fn checkout(target: Option<String>, create_branch: Option<String>, detach: bool, force: bool) -> anyhow::Result<()> {
    if create_branch.is_none() && target.is_none() {
        anyhow::bail!("fatal: you must specify a branch or commit to checkout");
    }

    if let Some(new_branch) = create_branch {
        let commit_hash = if let Some(start_point) = target {
            let branch_ref = format!("refs/heads/{}", start_point);
            if let Some(hash) = refs::read_ref(&branch_ref)? {
                hash
            } else {
                match read_object(&start_point) {
                    Ok((obj_type, _)) if obj_type == "commit" => start_point,
                    Ok((obj_type, _)) => {
                        anyhow::bail!("fatal: '{}' is not a commit (it is a {})", start_point, obj_type);
                    }
                    Err(_) => {
                        anyhow::bail!("fatal: '{}' is not a valid object hash or branch name", start_point);
                    }
                }
            }
        } else {
            refs::resolve_head_commit()?.ok_or_else(|| {
                anyhow::anyhow!("fatal: cannot create branch — no commits yet")
            })?
        };

        refs::create_branch(&new_branch, &commit_hash)?;
        switch(new_branch, false, false, force)?;
    } else if let Some(t) = target {
        let is_branch = {
            let branch_ref = format!("refs/heads/{}", t);
            refs::read_ref(&branch_ref)?.is_some()
        };
        let should_detach = detach || !is_branch;
        switch(t, false, should_detach, force)?;
    }

    Ok(())
}