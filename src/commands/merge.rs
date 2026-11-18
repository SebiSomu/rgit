use std::collections::BTreeMap;
use std::fs;
use crate::helpers::merge_tree::merge_base_tree;
use crate::helpers::merge::{die_if_unmerged, merge_into_worktree};
use crate::helpers::{build_merge_commit, build_tree_from_map, check_switch_safety, find_all_merge_bases, find_merge_base, is_reachable, resolve_commit_from_source, resolve_tree_from_source, sync_working_tree, update_index_from_tree};
use crate::index::read_index;
use crate::refs;

/// `rgit merge <branch>`
///
/// Fast-forwards when possible; otherwise merges `<branch>` into the current branch with the
/// same line-level three-way merge as `merge-tree` (merge bases of criss-cross histories are
/// merged into a virtual base first). Refuses to start while a merge is in progress or when
/// local changes would be overwritten. A clean merge is committed with the configured
/// identity. On conflicts, conflicted files get markers in the working tree, everything that
/// merged cleanly is staged, and `MERGE_HEAD`/`MERGE_MSG` are written so `rgit commit`
/// records the merge once the conflicts are resolved and added.
pub fn merge(branch: String) -> anyhow::Result<()> {
    die_if_unmerged("Merging", "Exiting because of an unresolved conflict.")?;
    if crate::repo::path(".git/MERGE_HEAD").exists() {
        eprintln!("fatal: You have not concluded your merge (MERGE_HEAD exists).\nPlease, commit your changes before you merge.");
        std::process::exit(128);
    }
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

    let (base_tree, _bases) = merge_base_tree(&our_commit, &their_commit, false)?;
    let our_tree = resolve_tree_from_source(&our_commit)?;
    let their_tree = resolve_tree_from_source(&their_commit)?;
    let result = merge_into_worktree(&base_tree, &our_tree, &their_tree, "HEAD", &branch, "merge")?;

    fs::write(crate::repo::path(".git/ORIG_HEAD"), format!("{}\n", our_commit))?;
    for message in &result.messages {
        println!("{}", message.text);
    }
    let merge_msg = merge_message(&branch);

    if !result.conflicted.is_empty() {
        // Clean paths are staged; conflicted ones hold their stages 1-3 in the index and the
        // marker content in the working tree.
        let mut msg = format!("{}\n\n# Conflicts:\n", merge_msg);
        for path in result.conflicted.keys() {
            msg.push_str(&format!("#\t{}\n", path));
        }
        fs::write(crate::repo::path(".git/MERGE_HEAD"), format!("{}\n", their_commit))?;
        fs::write(crate::repo::path(".git/MERGE_MSG"), msg)?;
        println!("Automatic merge failed; fix conflicts and then commit the result.");
        std::process::exit(1);
    }

    let tree_hash = build_tree_from_map(&result.tree)?;
    let commit_hash = build_merge_commit(tree_hash, &our_commit, &their_commit, &merge_msg)?;
    refs::write_ref(&format!("refs/heads/{}", current_branch), &commit_hash)?;

    println!("Merge made by the 'ort' strategy.");
    println!("[{} {}] {}", current_branch, &commit_hash[..7], merge_msg);
    Ok(())
}

/// git's default merge message: `Merge branch 'x'`, `Merge remote-tracking branch 'origin/x'`,
/// `Merge tag 'v1'`, or `Merge commit '<rev>'` for anything that isn't a branch or tag.
fn merge_message(name: &str) -> String {
    let exists = |r: String| refs::read_ref(&r).ok().flatten().is_some();
    // Same lookup order as revision parsing: `heads/x` / `tags/x` / `remotes/x` first, then a
    // tag wins over a branch of the same name.
    let kind = if name.starts_with("heads/") && exists(format!("refs/{}", name)) {
        "branch"
    } else if name.starts_with("tags/") && exists(format!("refs/{}", name)) {
        "tag"
    } else if name.starts_with("remotes/") && exists(format!("refs/{}", name)) {
        "remote-tracking branch"
    } else if exists(format!("refs/tags/{}", name)) {
        "tag"
    } else if exists(format!("refs/heads/{}", name)) {
        "branch"
    } else if exists(format!("refs/remotes/{}", name)) {
        "remote-tracking branch"
    } else {
        "commit"
    };
    format!("Merge {} '{}'", kind, name)
}

pub fn merge_base(all: bool, is_ancestor: bool, _octopus: bool, commits: Vec<String>) -> anyhow::Result<()> {
    if is_ancestor {
        if commits.len() != 2 {
            anyhow::bail!("fatal: --is-ancestor takes exactly two commits");
        }
        let commit1 = resolve_commit_from_source(&commits[0])?;
        let commit2 = resolve_commit_from_source(&commits[1])?;
        if is_reachable(&commit2, &commit1)? {
            return Ok(());
        } else {
            std::process::exit(1);
        }
    }

    if commits.len() < 2 {
        anyhow::bail!("fatal: merge-base requires at least two commits");
    }

    let resolved: Vec<String> = commits
        .iter()
        .map(|c| resolve_commit_from_source(c))
        .collect::<Result<_, _>>()?;

    if all {
        let mut current_bases = vec![resolved[0].clone()];
        for next_commit in &resolved[1..] {
            let mut next_round_bases = Vec::new();
            for base in &current_bases {
                let bases = find_all_merge_bases(base, next_commit)?;
                for b in bases {
                    if !next_round_bases.contains(&b) {
                        next_round_bases.push(b);
                    }
                }
            }

            let mut maximal_bases = Vec::new();
            for (i, c1) in next_round_bases.iter().enumerate() {
                let mut dominated = false;
                for (j, c2) in next_round_bases.iter().enumerate() {
                    if i != j && is_reachable(c2, c1)? {
                        dominated = true;
                        break;
                    }
                }
                if !dominated && !maximal_bases.contains(c1) {
                    maximal_bases.push(c1.clone());
                }
            }
            current_bases = maximal_bases;
            if current_bases.is_empty() {
                std::process::exit(1);
            }
        }

        for base in current_bases {
            println!("{}", base);
        }
    } else {
        let mut current_base = resolved[0].clone();
        for next_commit in &resolved[1..] {
            match find_merge_base(&current_base, next_commit)? {
                Some(base) => current_base = base,
                None => std::process::exit(1),
            }
        }
        println!("{}", current_base);
    }

    Ok(())
}
