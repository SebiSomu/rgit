use std::collections::BTreeMap;
use crate::helpers::{
    checkout::{check_switch_safety, sync_working_tree, update_index_from_tree},
    commit::is_reachable,
    objects::{flatten_tree, read_object, tree_hash_of_commit},
};
use crate::helpers::worktree::worktree_using_branch;
use crate::index::read_index;
use crate::refs;


/// `rgit branch [<options>] [<args>...]`
///
/// Lists, creates, deletes, renames and copies branches, like git's `branch`:
/// - list (no args, `-l`, `-a`, `-r`, `-v`/`-vv`, patterns, `--merged`/`--no-merged`,
///   `--contains`/`--no-contains`, `--points-at`), `--show-current`
/// - `<name> [<start>]` (`-f`, `-t`/`--track`, `--no-track`; a remote-tracking start point
///   sets the upstream automatically)
/// - `-d`/`-D` (`-r` for remote-tracking branches), checked against the upstream or HEAD
/// - `-m`/`-M` and `-c`/`-C` (`branch.<name>.*` config follows the branch)
/// - `-u`/`--set-upstream-to <upstream> [<branch>]`, `--unset-upstream [<branch>]`
pub fn branch(args: Vec<String>) -> anyhow::Result<()> {
    use crate::helpers::branch::*;
    let opts = parse_branch_args(&args);
    let has_filter = !(opts.merged.is_empty() && opts.no_merged.is_empty() && opts.contains.is_empty() && opts.no_contains.is_empty() && opts.points_at.is_empty());
    let mut list = opts.list || has_filter;
    if !opts.delete && !opts.rename && !opts.copy && opts.set_upstream.is_none() && !opts.unset_upstream && !opts.show_current && opts.args.is_empty() {
        list = true;
    }
    let modes = [opts.delete, opts.rename, opts.copy, opts.set_upstream.is_some(), opts.unset_upstream, opts.show_current, list];
    if modes.iter().filter(|m| **m).count() > 1 {
        eprint!("{}", BRANCH_USAGE);
        crate::ui::exit(129);
    }
    let head = refs::resolve_head()?;
    let current = match &head {
        refs::HeadState::Branch(b) => Some(b.clone()),
        refs::HeadState::Detached(_) => None,
    };

    if opts.show_current {
        if let Some(b) = current {
            println!("{}", b);
        }
        return Ok(());
    }
    if list {
        print!("{}", format_branch_list(&opts)?);
        return Ok(());
    }
    if opts.delete {
        return delete_branches(&opts);
    }
    if opts.rename || opts.copy {
        return rename_or_copy(&opts, current.as_deref());
    }
    if let Some(upstream) = &opts.set_upstream {
        if opts.args.len() > 1 {
            die("too many arguments to set new upstream");
        }
        let branch_name = match opts.args.first().cloned().or(current.clone()) {
            Some(b) => b,
            None => die(&format!("could not set upstream of HEAD to {} when it does not point to any branch", upstream)),
        };
        if refs::read_ref(&format!("refs/heads/{}", branch_name))?.is_none() {
            die(&format!("branch '{}' does not exist", branch_name));
        }
        let Some((remote, merge, display)) = upstream_for(upstream) else {
            eprintln!("fatal: the requested upstream branch '{}' does not exist", upstream);
            eprintln!("hint: \nhint: If you are planning on basing your work on an upstream\nhint: branch that already exists at the remote, you may need to\nhint: run \"rgit fetch\" to retrieve it.\nhint: \nhint: If you are planning to push out a new local branch that\nhint: will track its remote counterpart, you may want to use\nhint: \"rgit push -u\" to set the upstream config as you push.");
            crate::ui::exit(128);
        };
        if remote == "." && merge == format!("refs/heads/{}", branch_name) {
            eprintln!("warning: not setting branch '{}' as its own upstream", branch_name);
            return Ok(());
        }
        set_branch_upstream(&branch_name, &remote, &merge)?;
        if !opts.quiet {
            println!("branch '{}' set up to track '{}'.", branch_name, display);
        }
        return Ok(());
    }
    if opts.unset_upstream {
        if opts.args.len() > 1 {
            die("too many arguments to unset upstream");
        }
        let branch_name = match opts.args.first().cloned().or(current.clone()) {
            Some(b) => b,
            None => die("could not unset upstream of HEAD when it does not point to any branch"),
        };
        if refs::read_ref(&format!("refs/heads/{}", branch_name))?.is_none() {
            die(&format!("branch '{}' does not exist", branch_name));
        }
        if !unset_branch_upstream(&branch_name)? {
            die(&format!("branch '{}' has no upstream information", branch_name));
        }
        return Ok(());
    }

    // Creating a branch.
    if opts.remotes || opts.all {
        die("the -a, and -r, options to 'rgit branch' do not take a branch name.\nDid you mean to use: -a|-r --list <pattern>?");
    }
    if opts.args.len() > 2 {
        eprint!("{}", BRANCH_USAGE);
        crate::ui::exit(129);
    }
    let name = &opts.args[0];
    let start = opts.args.get(1).cloned().unwrap_or_else(|| "HEAD".to_string());
    let commit = match crate::helpers::rev_parse::resolve_rev(&format!("{}^{{commit}}", start)) {
        Ok(c) => c,
        Err(_) if opts.args.len() == 1 => die(&format!("not a valid object name: '{}'", current.as_deref().unwrap_or("HEAD"))),
        Err(_) => die(&format!("not a valid object name: '{}'", start)),
    };
    let refname = format!("refs/heads/{}", name);
    if refs::read_ref(&refname)?.is_some() {
        if !opts.force {
            die(&format!("a branch named '{}' already exists", name));
        }
        if let Some(user) = worktree_using_branch(&refname, true)? {
            die(&format!("cannot force update the branch '{}' used by worktree at '{}'", name, user.display_path()));
        }
        if !refs::is_valid_branch_name(name) {
            die(&format!("'{}' is not a valid branch name", name));
        }
        refs::write_ref(&refname, &commit)?;
    } else {
        refs::create_branch(name, &commit)?;
    }

    let tracking = if opts.args.len() == 2 { upstream_for(&start) } else { None };
    let track = match opts.track {
        Some(t) => t,
        None => tracking.as_ref().is_some_and(|(remote, _, _)| remote != "."),
    };
    if track {
        let Some((remote, merge, display)) = tracking else {
            die(&format!("cannot set up tracking information; starting point '{}' is not a branch", start));
        };
        set_branch_upstream(name, &remote, &merge)?;
        if !opts.quiet {
            println!("branch '{}' set up to track '{}'.", name, display);
        }
    }
    Ok(())
}

/// `branch -d/-D [-r] <name>...`: deletes each branch (or remote-tracking branch), refusing
/// unmerged ones without `-D`, and reports every failure before exiting 1.
fn delete_branches(opts: &crate::objects::BranchOptions) -> anyhow::Result<()> {
    use crate::helpers::branch::*;
    if opts.args.is_empty() {
        die("branch name required");
    }
    let head_commit = refs::resolve_head_commit()?;
    let mut failed = false;
    for name in &opts.args {
        if opts.remotes {
            let refname = format!("refs/remotes/{}", name);
            let was = match symref_target(&refname) {
                Some(target) => Some(target),
                None => refs::read_ref(&refname)?.map(|h| h[..7].to_string()),
            };
            let Some(was) = was else {
                eprintln!("error: remote-tracking branch '{}' not found.", name);
                failed = true;
                continue;
            };
            delete_remote_tracking_ref(name)?;
            if !opts.quiet {
                println!("Deleted remote-tracking branch {} (was {}).", name, was);
            }
            continue;
        }
        let refname = format!("refs/heads/{}", name);
        let in_use = worktree_using_branch(&refname, true)?.map(|w| w.display_path());
        if let Some(path) = in_use {
            eprintln!("error: cannot delete branch '{}' used by worktree at '{}'", name, path);
            failed = true;
            continue;
        }
        let Some(tip) = refs::read_ref(&refname)? else {
            eprintln!("error: branch '{}' not found", name);
            failed = true;
            continue;
        };
        if !opts.force {
            // Merged into its upstream if it has one, else into HEAD (git's branch_merged).
            let upstream = crate::helpers::for_each_ref::branch_upstream(name).and_then(|u| refs::read_ref(&u).ok().flatten());
            let into = |base: &Option<String>| -> anyhow::Result<bool> {
                Ok(match base {
                    Some(b) => is_reachable(b, &tip)?,
                    None => false,
                })
            };
            let merged_head = into(&head_commit)?;
            let merged = match &upstream {
                Some(_) => {
                    let merged_up = into(&upstream)?;
                    let up_name = crate::helpers::for_each_ref::branch_upstream(name).unwrap_or_default();
                    if merged_up && !merged_head {
                        eprintln!("warning: deleting branch '{}' that has been merged to\n         '{}', but not yet merged to HEAD.", name, up_name);
                    } else if !merged_up && merged_head {
                        eprintln!("warning: not deleting branch '{}' that is not yet merged to\n         '{}', even though it is merged to HEAD.", name, up_name);
                    }
                    merged_up
                }
                None => merged_head,
            };
            if !merged {
                eprintln!("error: the branch '{}' is not fully merged.\nIf you are sure you want to delete it, run 'rgit branch -D {}'", name, name);
                failed = true;
                continue;
            }
        }
        refs::delete_branch(name)?;
        move_branch_config(name, None, false)?;
        if !opts.quiet {
            println!("Deleted branch {} (was {}).", name, &tip[..7]);
        }
    }
    if failed {
        crate::ui::exit(1);
    }
    Ok(())
}

/// `branch -m/-M/-c/-C [<old>] <new>`: renames or copies a branch (the current one when only
/// `<new>` is given) together with its `branch.<name>.*` config.
fn rename_or_copy(opts: &crate::objects::BranchOptions, current: Option<&str>) -> anyhow::Result<()> {
    use crate::helpers::branch::*;
    let verb = if opts.copy { "copy" } else { "rename" };
    let (old, new) = match opts.args.as_slice() {
        [] => die("branch name required"),
        [new] => match current {
            Some(c) => (c.to_string(), new.clone()),
            None => die(&format!("cannot {} the current branch while not on any", verb)),
        },
        [old, new] => (old.clone(), new.clone()),
        _ => die(&format!("too many arguments for a {} operation", verb)),
    };
    let old_ref = format!("refs/heads/{}", old);
    let new_ref = format!("refs/heads/{}", new);
    let Some(tip) = refs::read_ref(&old_ref)? else {
        die(&format!("no branch named '{}'", old));
    };
    if !refs::is_valid_branch_name(&new) {
        die(&format!("'{}' is not a valid branch name", new));
    }
    if old != new && refs::read_ref(&new_ref)?.is_some() {
        if !opts.force {
            die(&format!("a branch named '{}' already exists", new));
        }
        if let Some(user) = worktree_using_branch(&new_ref, true)? {
            die(&format!("cannot force update the branch '{}' used by worktree at '{}'", new, user.display_path()));
        }
        refs::delete_branch(&new)?;
    }
    if old == new && !opts.copy {
        return Ok(());
    }
    if opts.copy {
        if old != new {
            refs::write_ref(&new_ref, &tip)?;
        }
    } else {
        refs::rename_branch(&old, &new)?;
    }
    move_branch_config(&old, Some(&new), opts.copy)?;
    Ok(())
}

/// `switch` refuses to run in the middle of a merge, cherry-pick, revert, rebase or bisect,
/// as git's does (`checkout` doesn't check this).
// Used by the `switch` command.
pub fn die_if_operation_in_progress() -> anyhow::Result<()> {
    let state = |p: &str| crate::repo::path(p).exists();
    let (doing, quit) = if state(".git/MERGE_HEAD") {
        ("merging", "rgit reset --hard\" (which discards the merge)")
    } else if crate::helpers::rebase::rebase_in_progress() {
        ("rebasing", "rgit rebase --abort\"")
    } else if state(".git/CHERRY_PICK_HEAD") {
        ("cherry-picking", "rgit cherry-pick --abort\"")
    } else if state(".git/REVERT_HEAD") {
        ("reverting", "rgit revert --abort\"")
    } else if state(crate::commands::bisect::BISECT_START_PATH) {
        ("bisecting", "rgit bisect reset\"")
    } else {
        return Ok(());
    };
    eprintln!("fatal: cannot switch branch while {}", doing);
    // git suggests `--quit`; rgit points at the commands it has.
    eprintln!("Consider \"{} or \"rgit worktree add\".", quit);
    crate::ui::exit(128);
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
        if let Some(user) = worktree_using_branch(&format!("refs/heads/{}", branch), false)? {
            anyhow::bail!("fatal: '{}' is already used by worktree at '{}'", branch, user.display_path());
        }
    }

    let target_tree_hash = tree_hash_of_commit(&target_commit)?;
    let mut target_tree = BTreeMap::new();
    flatten_tree(&target_tree_hash, "", &mut target_tree)?;

    let mut head_tree = BTreeMap::new();
    let current_commit = refs::resolve_head_commit()?;
    if let Some(current_commit) = &current_commit {
        let current_tree_hash = tree_hash_of_commit(current_commit)?;
        flatten_tree(&current_tree_hash, "", &mut head_tree)?;
    }

    let index_entries = read_index().unwrap_or_default();
    let unmerged = crate::helpers::merge::unmerged_paths(&index_entries);
    if !unmerged.is_empty() {
        if !force && current_commit.as_deref() != Some(target_commit.as_str()) {
            // git's refusal: the error on stderr, then its refresh report on stdout.
            eprintln!("error: you need to resolve your current index first");
            for path in &unmerged {
                println!("{}: needs merge", path);
            }
            crate::ui::exit(1);
        }
        // `-f` throws the conflicts away: unmerged paths are rewritten from the target.
        for path in &unmerged {
            head_tree.remove(path);
        }
    }
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
