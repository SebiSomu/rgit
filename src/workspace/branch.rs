pub mod cli;
pub mod engine;
pub mod types;

use std::collections::BTreeMap;
use crate::workspace::checkout::{check_switch_safety, sync_working_tree, update_index_from_tree};
use crate::core::commit::is_reachable;
use crate::core::object_store::{flatten_tree, read_object, tree_hash_of_commit};
use crate::workspace::worktree::engine::worktree_using_branch;
use crate::core::index::read_index;
use crate::core::refs;

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
    use crate::workspace::branch::engine::{
        BRANCH_USAGE,
        format_branch_list,
        parse_branch_args,
        set_branch_upstream,
        unset_branch_upstream,
        upstream_for,
    };
    let opts = parse_branch_args(&args);
    if opts.push.is_some() && !opts.rename {
        crate::ui::die_fatal("--push only goes with -m/-M (rename a branch here and on its remote)");
    }
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
            crate::ui::die_fatal("too many arguments to set new upstream");
        }
        let branch_name = match opts.args.first().cloned().or(current.clone()) {
            Some(b) => b,
            None => crate::ui::die_fatal(format!("could not set upstream of HEAD to {} when it does not point to any branch", upstream)),
        };
        if refs::read_ref(&format!("refs/heads/{}", branch_name))?.is_none() {
            crate::ui::die_fatal(format!("branch '{}' does not exist", branch_name));
        }
        let Some((remote, merge, display)) = upstream_for(upstream) else {
            eprintln!("fatal: the requested upstream branch '{}' does not exist", upstream);
            crate::ui::eprintln_hints("hint: \nhint: If you are planning on basing your work on an upstream\nhint: branch that already exists at the remote, you may need to\nhint: run \"rgit fetch\" to retrieve it.\nhint: \nhint: If you are planning to push out a new local branch that\nhint: will track its remote counterpart, you may want to use\nhint: \"rgit push -u\" to set the upstream config as you push.");
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
            crate::ui::die_fatal("too many arguments to unset upstream");
        }
        let branch_name = match opts.args.first().cloned().or(current.clone()) {
            Some(b) => b,
            None => crate::ui::die_fatal("could not unset upstream of HEAD when it does not point to any branch"),
        };
        if refs::read_ref(&format!("refs/heads/{}", branch_name))?.is_none() {
            crate::ui::die_fatal(format!("branch '{}' does not exist", branch_name));
        }
        if !unset_branch_upstream(&branch_name)? {
            crate::ui::die_fatal(format!("branch '{}' has no upstream information", branch_name));
        }
        return Ok(());
    }

    // Creating a branch.
    if opts.remotes || opts.all {
        crate::ui::die_fatal("the -a, and -r, options to 'rgit branch' do not take a branch name.\nDid you mean to use: -a|-r --list <pattern>?");
    }
    if opts.args.len() > 2 {
        eprint!("{}", BRANCH_USAGE);
        crate::ui::exit(129);
    }
    let name = &opts.args[0];
    let start = opts.args.get(1).cloned().unwrap_or_else(|| "HEAD".to_string());
    let commit = match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", start)) {
        Ok(c) => c,
        Err(_) if opts.args.len() == 1 => crate::ui::die_fatal(format!("not a valid object name: '{}'", current.as_deref().unwrap_or("HEAD"))),
        Err(_) => crate::ui::die_fatal(format!("not a valid object name: '{}'", start)),
    };
    let refname = format!("refs/heads/{}", name);
    if refs::read_ref(&refname)?.is_some() {
        if !opts.force {
            crate::ui::die_fatal(format!("a branch named '{}' already exists", name));
        }
        if let Some(user) = worktree_using_branch(&refname, true)? {
            crate::ui::die_fatal(format!("cannot force update the branch '{}' used by worktree at '{}'", name, user.display_path()));
        }
        if !refs::is_valid_branch_name(name) {
            crate::ui::die_fatal(format!("'{}' is not a valid branch name", name));
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
            crate::ui::die_fatal(format!("cannot set up tracking information; starting point '{}' is not a branch", start));
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
fn delete_branches(opts: &crate::workspace::branch::types::BranchOptions) -> anyhow::Result<()> {
    use crate::workspace::branch::engine::{
        delete_remote_tracking_ref,
        move_branch_config,
        symref_target,
    };
    if opts.args.is_empty() {
        crate::ui::die_fatal("branch name required");
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
            let upstream = crate::references::for_each_ref::engine::upstream::branch_upstream(name).and_then(|u| refs::read_ref(&u).ok().flatten());
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
                    let up_name = crate::references::for_each_ref::engine::upstream::branch_upstream(name).unwrap_or_default();
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
fn rename_or_copy(opts: &crate::workspace::branch::types::BranchOptions, current: Option<&str>) -> anyhow::Result<()> {
    use crate::workspace::branch::engine::move_branch_config;
    let verb = if opts.copy { "copy" } else { "rename" };
    let (old, new) = match opts.args.as_slice() {
        [] => crate::ui::die_fatal("branch name required"),
        [new] => match current {
            Some(c) => (c.to_string(), new.clone()),
            None => crate::ui::die_fatal(format!("cannot {} the current branch while not on any", verb)),
        },
        [old, new] => (old.clone(), new.clone()),
        _ => crate::ui::die_fatal(format!("too many arguments for a {} operation", verb)),
    };
    let old_ref = format!("refs/heads/{}", old);
    let new_ref = format!("refs/heads/{}", new);
    let Some(tip) = refs::read_ref(&old_ref)? else {
        crate::ui::die_fatal(format!("no branch named '{}'", old));
    };
    if !refs::is_valid_branch_name(&new) {
        crate::ui::die_fatal(format!("'{}' is not a valid branch name", new));
    }
    if old != new && refs::read_ref(&new_ref)?.is_some() {
        if !opts.force {
            crate::ui::die_fatal(format!("a branch named '{}' already exists", new));
        }
        if let Some(user) = worktree_using_branch(&new_ref, true)? {
            crate::ui::die_fatal(format!("cannot force update the branch '{}' used by worktree at '{}'", new, user.display_path()));
        }
        refs::delete_branch(&new)?;
    }
    if old == new && !opts.copy {
        return Ok(());
    }
    // `--push`: work out (and show) the remote side before the config moves to the new name.
    let remote_plan = match &opts.push {
        Some(remote) => Some(plan_remote_rename(&old, &new, remote.as_deref())?),
        None => None,
    };
    if opts.copy {
        if old != new {
            refs::write_ref(&new_ref, &tip)?;
        }
    } else {
        refs::rename_branch(&old, &new)?;
    }
    move_branch_config(&old, Some(&new), opts.copy)?;
    if let Some(plan) = remote_plan {
        rename_on_remote(&old, &new, &plan)?;
    }
    Ok(())
}

/// Picks the remote (`--push=<remote>`, else `branch.<old>.remote`) and the remote branch to
/// delete (the upstream when it is on that remote, else `<old>`; only when rgit knows it exists:
/// an upstream is configured or a remote-tracking ref is there), then prints the plan.
// Used by `rename_or_copy` (`-m --push`).
fn plan_remote_rename(old: &str, new: &str, remote: Option<&str>) -> anyhow::Result<crate::workspace::branch::types::RemoteRename> {
    use crate::core::config::get_config_value;
    let configured = get_config_value(&format!("branch.{}.remote", old))?;
    let remote = match remote.map(str::to_string).or_else(|| configured.clone()) {
        Some(r) => r,
        None => crate::ui::die_fatal(format!("branch '{}' has no upstream to rename it on\nhint: name the remote: rgit branch -m {} {} --push=<remote>", old, old, new)),
    };
    if get_config_value(&format!("remote.{}.url", remote))?.is_none() {
        crate::ui::die_fatal(format!("'{}' is not a remote (see `rgit remote -v`)", remote));
    }
    let upstream = (configured.as_deref() == Some(remote.as_str()))
        .then(|| get_config_value(&format!("branch.{}.merge", old)).ok().flatten())
        .flatten()
        .and_then(|m| m.strip_prefix("refs/heads/").map(str::to_string));
    let old_remote_branch = upstream.clone().unwrap_or_else(|| old.to_string());
    let tracked = refs::read_ref(&format!("refs/remotes/{}/{}", remote, old_remote_branch))?.is_some();
    let delete_old = (upstream.is_some() || tracked) && old_remote_branch != new;

    println!("Renaming '{}' to '{}' here and on '{}':", old, new, remote);
    println!("  1. rename the local branch (its reflog and config come along)");
    println!("  2. push '{}' to {} and track {}/{}", new, remote, remote, new);
    if delete_old {
        println!("  3. delete '{}' on {}", old_remote_branch, remote);
    } else {
        println!("  ({} has no '{}' that rgit knows of: nothing to delete there)", remote, old_remote_branch);
    }
    Ok(crate::workspace::branch::types::RemoteRename { remote, old_remote_branch, delete_old })
}

/// The remote half of `branch -m --push`, after the local rename: push the new name, track it,
/// delete the old name. The new name goes first, so a failure never leaves the remote without
/// the branch; on a failure the commands that are left are printed.
// Used by `rename_or_copy` (`-m --push`).
fn rename_on_remote(old: &str, new: &str, plan: &crate::workspace::branch::types::RemoteRename) -> anyhow::Result<()> {
    let left = |from: usize| {
        let mut steps = vec![format!("rgit push -u {} {}", plan.remote, new)];
        if plan.delete_old {
            steps.push(format!("rgit push {} :{}", plan.remote, plan.old_remote_branch));
        }
        eprintln!("The local branch is renamed to '{}'; to finish on '{}', run:", new, plan.remote);
        for step in &steps[from..] {
            eprintln!("  {}", step);
        }
    };
    if let Err(e) = crate::remote::push::push(Some(plan.remote.clone()), Some(format!("{}:{}", new, new)), false, false) {
        eprintln!("{}", e);
        left(0);
        crate::ui::exit(1);
    }
    crate::remote::push::engine::set_upstream_branch(new, &plan.remote, new)?;
    println!("branch '{}' set up to track '{}/{}'.", new, plan.remote, new);
    if plan.delete_old
        && let Err(e) = crate::remote::push::push(Some(plan.remote.clone()), Some(format!(":{}", plan.old_remote_branch)), false, false)
    {
        eprintln!("{}", e);
        left(1);
        crate::ui::exit(1);
    }
    println!("Renamed '{}' to '{}' here and on '{}'. (`rgit undo` only renames it back here.)", old, new, plan.remote);
    Ok(())
}

/// `switch` refuses to run in the middle of a merge, cherry-pick, revert, rebase or bisect,
/// as git's does (`checkout` doesn't check this).
// Used by the `switch` command.
pub fn die_if_operation_in_progress() -> anyhow::Result<()> {
    let state = |p: &str| crate::core::repo::path(p).exists();
    let (doing, quit) = if state(".git/MERGE_HEAD") {
        ("merging", "rgit reset --hard\" (which discards the merge)")
    } else if crate::merge::rebase::engine::rebase_in_progress() {
        ("rebasing", "rgit rebase --abort\"")
    } else if state(".git/CHERRY_PICK_HEAD") {
        ("cherry-picking", "rgit cherry-pick --abort\"")
    } else if state(".git/REVERT_HEAD") {
        ("reverting", "rgit revert --abort\"")
    } else if state(crate::history::bisect::engine::BISECT_START_PATH) {
        ("bisecting", "rgit bisect reset\"")
    } else {
        return Ok(());
    };
    eprintln!("fatal: cannot switch branch while {}", doing);
    // git suggests `--quit`; rgit points at the commands it has.
    eprintln!("Consider \"{} or \"rgit worktree add\".", quit);
    crate::ui::exit(128);
}

pub fn switch(branch: String, create: bool, detach: bool, force: bool, start_point: Option<String>) -> anyhow::Result<()> {
    // `-` is where you were before the last switch, like git's `@{-1}`.
    let branch = if branch == "-" { previous_checkout()? } else { branch };
    let mut created = create;
    if create {
        let commit_hash = match &start_point {
            Some(start) => resolve_target_commit(start)?,
            None => refs::resolve_head_commit()?.ok_or_else(|| {
                anyhow::anyhow!("fatal: cannot create branch — no commits yet")
            })?,
        };
        refs::create_branch(&branch, &commit_hash)?;
    } else if !detach && refs::read_ref(&format!("refs/heads/{}", branch))?.is_none() {
        // git's guess: `switch feature` with only `origin/feature` creates a tracking branch.
        if let Some((remote, hash)) = guess_remote_branch(&branch) {
            refs::create_branch(&branch, &hash)?;
            crate::workspace::branch::engine::set_branch_upstream(&branch, &remote, &format!("refs/heads/{}", branch))?;
            println!("branch '{}' set up to track '{}/{}'.", branch, remote, branch);
            created = true;
        }
    }

    let target_commit = if detach {
        resolve_target_commit(&branch)?
    } else {
        let target_ref = format!("refs/heads/{}", branch);
        match refs::read_ref(&target_ref)? {
            Some(hash) => hash,
            None => {
                if resolve_target_commit(&branch).is_ok() {
                    eprintln!("fatal: a branch is expected, got commit '{}'", branch);
                    crate::ui::eprintln_hints("hint: If you want to detach HEAD at the commit, try again with the --detach option.");
                    crate::ui::exit(128);
                }
                anyhow::bail!("fatal: invalid reference: {}", branch)
            }
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
    let unmerged = crate::merge::merge::engine::unmerged_paths(&index_entries);
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

    // git's HEAD reflog entry, which `checkout -` / `switch -` read back.
    let from = match refs::resolve_head()? {
        refs::HeadState::Branch(b) => b,
        refs::HeadState::Detached(h) => h,
    };
    crate::core::reflog::set_reflog_action(&format!("checkout: moving from {} to {}", from, branch));
    if detach {
        refs::set_head_detached(&target_commit)?;
        println!("HEAD is now at {} (detached)", &target_commit[..7]);
        eprintln!("warning: You are in a detached HEAD state.");
        eprintln!("  You can look around, make experimental commits, or switch -c <branch> to keep them.");
    } else if created {
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
    let target = match target.as_deref() {
        Some("-") => Some(previous_checkout()?),
        _ => target,
    };

    if let Some(new_branch) = create_branch {
        // `-b <new> [<start-point>]`: the start point can be any revision.
        return switch(new_branch, true, false, force, target);
    }
    let Some(t) = target else { return Ok(()) };
    let is_branch = refs::read_ref(&format!("refs/heads/{}", t))?.is_some();
    if is_branch {
        return switch(t, false, detach, force, None);
    }
    // `checkout HEAD` stays where it is, as in git.
    if t == "HEAD" && !detach {
        return Ok(());
    }
    // Any revision (tag, `HEAD~1`, short hash, `origin/main`, ...) is checked out detached;
    // a name only a remote has becomes a new tracking branch.
    match resolve_target_commit(&t) {
        Ok(_) => switch(t, false, true, force, None),
        Err(e) => {
            if !detach && guess_remote_branch(&t).is_some() {
                switch(t, false, false, force, None)
            } else {
                Err(e)
            }
        }
    }
}

/// Resolves what `checkout` / `switch --detach` / a start point names to a commit: a local
/// branch first, then any revision rgit's revision parser understands (tags, `HEAD~1`,
/// `main^2`, abbreviated hashes, `origin/main`, `@{1}`), peeling annotated tags.
// Used by `switch` and `checkout`.
fn resolve_target_commit(name: &str) -> anyhow::Result<String> {
    if let Some(hash) = refs::read_ref(&format!("refs/heads/{}", name))? {
        return Ok(hash);
    }
    let hash = crate::core::rev_parse::resolve_rev(name).map_err(|_| anyhow::anyhow!("fatal: '{}' is not a valid object hash or branch name", name))?;
    match crate::core::rev_parse::peel_to_type(hash.clone(), "commit") {
        Ok(commit) => Ok(commit),
        Err(_) => {
            let (obj_type, _) = read_object(&hash)?;
            anyhow::bail!("fatal: '{}' is not a commit (it is a {})", name, obj_type)
        }
    }
}

/// The single remote-tracking branch `<remote>/<name>` (not `<remote>/HEAD`), for git's
/// "checkout a branch only the remote has" guess; None when there is none or several.
// Used by `switch` and `checkout`.
fn guess_remote_branch(name: &str) -> Option<(String, String)> {
    if name == "HEAD" {
        return None;
    }
    let matches: Vec<(String, String)> = refs::list_all_refs()
        .into_iter()
        .filter_map(|(r, hash)| {
            let rest = r.strip_prefix("refs/remotes/")?;
            let (remote, branch) = rest.split_once('/')?;
            (branch == name).then(|| (remote.to_string(), hash))
        })
        .collect();
    if matches.len() == 1 { matches.into_iter().next() } else { None }
}

/// What `-` means for `checkout` / `switch`: the branch (or commit, if HEAD was detached) you
/// were on before the last switch, read from HEAD's reflog like git's `@{-1}`.
// Used by `switch` and `checkout`.
fn previous_checkout() -> anyhow::Result<String> {
    let entries = crate::core::reflog::read_reflog("HEAD").unwrap_or_default();
    entries
        .iter()
        .find_map(|e| e.message.strip_prefix("checkout: moving from ")?.split_once(" to ").map(|(from, _)| from.to_string()))
        .ok_or_else(|| anyhow::anyhow!("fatal: no previous branch to go back to ('-' means the branch you were on before the last switch)"))
}
