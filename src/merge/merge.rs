pub mod cli;
pub mod engine;
pub mod types;

use std::collections::BTreeMap;
use std::fs;
use crate::merge::merge_tree::engine::merge_base_tree;
use crate::merge::merge::engine::{abort_merge_to_head, clear_merge_state, die_if_unmerged, merge_into_worktree_favoring};
use crate::workspace::checkout::{check_switch_safety, sync_working_tree, update_index_from_tree};
use crate::workspace::state_files::build_tree_from_map;
use crate::merge::merge::engine::{find_all_merge_bases, find_merge_base};
use crate::core::commit::{is_reachable, resolve_commit_from_source, resolve_tree_from_source};
use crate::core::index::read_index;
use crate::merge::merge::types::{FastForwardMode, MergeFavor, MergeOptions};
use crate::core::refs;

type TreeMap = BTreeMap<String, ([u8; 20], u32)>;

fn die(code: i32, message: &str) -> ! {
    eprintln!("{}", message);
    crate::ui::exit(code);
}

/// `rgit merge [<options>] <commit>` / `--abort` / `--continue` / `--quit`
///
/// Fast-forwards when possible (`--ff`, the default), always records a merge commit with
/// `--no-ff`, or refuses anything but a fast-forward with `--ff-only`. Otherwise merges
/// `<commit>` into `HEAD` with the same line-level three-way merge as `merge-tree` (a virtual
/// base for criss-cross histories), `-X ours`/`-X theirs` resolving content conflicts toward
/// one side, or `-s ours` keeping `HEAD`'s tree as is. A clean merge is committed (message
/// from `-m`/`-F`, else git's default) and followed by a diffstat (`-n`/`-q` turn it off);
/// `--no-commit` stops before committing, and `--squash` leaves the merged result staged with
/// a `SQUASH_MSG` and no merge recorded. On conflicts the index holds stages 1-3, and
/// `MERGE_HEAD`/`MERGE_MSG`/`MERGE_MODE`/`AUTO_MERGE` are written so `rgit commit` (or
/// `merge --continue`) records the merge; `--abort` puts back `HEAD` the way `reset --merge`
/// does, and `--quit` forgets the merge but keeps the index and working tree. Messages,
/// streams and exit codes follow git 2.43.
pub fn merge(opts: MergeOptions) -> anyhow::Result<()> {
    if let Some(strategy) = &opts.strategy {
        if !["ort", "recursive", "ours", "resolve", "octopus", "subtree"].contains(&strategy.as_str()) {
            die(1, &format!("Could not find merge strategy '{}'.\nAvailable strategies are: octopus ours recursive resolve subtree.", strategy));
        }
        if !["ort", "recursive", "ours"].contains(&strategy.as_str()) {
            die(1, &format!("fatal: merge strategy '{}' is not supported by rgit (use ort, recursive or ours)", strategy));
        }
    }
    let merge_head = crate::core::repo::path(".git/MERGE_HEAD");
    if opts.abort || opts.quit || opts.cont {
        let flag = if opts.abort { "--abort" } else if opts.quit { "--quit" } else { "--continue" };
        if !opts.commits.is_empty() {
            die(129, &format!("fatal: {} expects no arguments", flag));
        }
        if opts.quit {
            clear_merge_state();
            return Ok(());
        }
        if !merge_head.exists() {
            die(128, if opts.abort { "fatal: There is no merge to abort (MERGE_HEAD missing)." } else { "fatal: There is no merge in progress (MERGE_HEAD missing)." });
        }
        if opts.abort {
            return abort_merge_to_head();
        }
        return crate::workspace::commit::commit(Default::default());
    }

    die_if_unmerged("Merging", "Exiting because of an unresolved conflict.")?;
    if merge_head.exists() {
        die(128, "fatal: You have not concluded your merge (MERGE_HEAD exists).\nPlease, commit your changes before you merge.");
    }
    if crate::core::repo::path(".git/CHERRY_PICK_HEAD").exists() {
        die(128, "fatal: You have not concluded your cherry-pick (CHERRY_PICK_HEAD exists).\nPlease, commit your changes before you merge.");
    }
    if opts.squash {
        if opts.ff == FastForwardMode::Never {
            die(128, "fatal: options '--squash' and '--no-ff.' cannot be used together");
        }
        if opts.commit == Some(true) {
            die(128, "fatal: options '--squash' and '--commit.' cannot be used together");
        }
    }
    match opts.commits.len() {
        0 => die(128, "fatal: No remote for the current branch."),
        1 => {}
        _ => die(128, "fatal: rgit merge takes a single commit (octopus merges are not supported)"),
    }
    let name = &opts.commits[0];
    let strategy = opts.strategy.clone().unwrap_or_else(|| "ort".to_string());
    let show_stat = !opts.no_stat && !opts.quiet;

    let head_state = refs::resolve_head()?;
    let our_commit = refs::resolve_head_commit()?.ok_or_else(|| anyhow::anyhow!("fatal: HEAD has no commits yet"))?;
    let their_commit = match resolve_commit_from_source(name) {
        Ok(c) => c,
        Err(_) => die(1, &format!("merge: {} - not something we can merge", name)),
    };
    let message = merge_commit_message(&opts, name, &head_state)?;
    fs::write(crate::core::repo::path(".git/ORIG_HEAD"), format!("{}\n", our_commit))?;

    if our_commit == their_commit || is_reachable(&our_commit, &their_commit)? {
        println!("{}", if opts.squash { "Already up to date. (nothing to squash)" } else { "Already up to date." });
        return Ok(());
    }

    let can_ff = strategy != "ours" && is_reachable(&their_commit, &our_commit)?;
    if can_ff && opts.ff != FastForwardMode::Never {
        let our_tree = resolve_tree_from_source(&our_commit)?;
        let target_tree = resolve_tree_from_source(&their_commit)?;
        if !opts.quiet {
            println!("Updating {}..{}", &our_commit[..7], &their_commit[..7]);
        }
        let index_entries = read_index().unwrap_or_default();
        let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();
        if let Err(e) = check_switch_safety(&target_tree, &our_tree, &index_map) {
            die(1, &for_merge(&e.to_string()));
        }
        sync_working_tree(&target_tree, &our_tree, &index_map)?;
        update_index_from_tree(&target_tree, &our_tree)?;
        if !opts.quiet {
            println!("Fast-forward");
        }
        if opts.squash {
            println!("Squash commit -- not updating HEAD");
            write_squash_message(&our_commit, &their_commit)?;
        } else {
            crate::core::reflog::set_reflog_action(&format!("merge {}: Fast-forward", name));
            move_head(&head_state, &their_commit)?;
        }
        if show_stat {
            print_diffstat(&our_tree, &target_tree)?;
        }
        return Ok(());
    }
    if opts.ff == FastForwardMode::Only {
        eprintln!("hint: Diverging branches can't be fast-forwarded, you need to either:\nhint: \nhint: \trgit merge --no-ff\nhint: \nhint: or:\nhint: \nhint: \trgit rebase\nhint: \nhint: Disable this message with \"rgit config advice.diverging false\"");
        die(128, "fatal: Not possible to fast-forward, aborting.");
    }

    let our_tree = resolve_tree_from_source(&our_commit)?;
    let favor = favor_from_options(&opts.strategy_options);
    let result = if strategy == "ours" {
        crate::merge::types::TreeMergeResult { tree: our_tree.clone(), ..Default::default() }
    } else {
        let (base_tree, _bases) = match merge_base_tree(&our_commit, &their_commit, opts.allow_unrelated) {
            Ok(base) => base,
            Err(e) => die(128, &e.to_string()),
        };
        let their_tree = resolve_tree_from_source(&their_commit)?;
        match merge_into_worktree_favoring(&base_tree, &our_tree, &their_tree, "HEAD", name, "merge", favor) {
            Ok(result) => result,
            Err(e) => die(2, &e.to_string()),
        }
    };
    for line in &result.messages {
        println!("{}", line.text);
    }
    let auto_merge = build_tree_from_map(&result.tree)?;
    let conflicted = !result.conflicted.is_empty();

    if opts.squash {
        println!("Squash commit -- not updating HEAD");
        write_squash_message(&our_commit, &their_commit)?;
    }
    if conflicted || opts.squash || opts.commit == Some(false) {
        // Only ort records its result tree.
        if strategy == "ort" {
            fs::write(crate::core::repo::path(".git/AUTO_MERGE"), format!("{}\n", auto_merge))?;
        }
        let mut msg = if opts.squash { String::new() } else { message.clone() };
        if conflicted {
            msg.push_str("\n# Conflicts:\n");
            for path in result.conflicted.keys() {
                msg.push_str(&format!("#\t{}\n", path));
            }
        }
        if !opts.squash {
            fs::write(&merge_head, format!("{}\n", their_commit))?;
            fs::write(crate::core::repo::path(".git/MERGE_MODE"), if opts.ff == FastForwardMode::Never || strategy == "ours" { "no-ff" } else { "" })?;
        }
        if conflicted || !opts.squash {
            fs::write(crate::core::repo::path(".git/MERGE_MSG"), msg)?;
        }
        if conflicted {
            println!("Automatic merge failed; fix conflicts and then commit the result.");
            crate::ui::exit(1);
        }
        eprintln!("Automatic merge went well; stopped before committing as requested");
        return Ok(());
    }

    let signature = crate::core::commit::current_signature()?;
    let commit_hash = crate::core::commit::write_commit_object(&auto_merge, &[our_commit.clone(), their_commit.clone()], &signature, &signature, &message)?;
    let made_by = format!("Merge made by the '{}' strategy.", strategy);
    crate::core::reflog::set_reflog_action(&format!("merge {}: {}", name, made_by));
    move_head(&head_state, &commit_hash)?;
    if !opts.quiet {
        println!("{}", made_by);
    }
    if show_stat {
        print_diffstat(&our_tree, &result.tree)?;
    }
    Ok(())
}

/// `-X ours` / `-X theirs` (the last one wins); any other option is refused.
fn favor_from_options(options: &[String]) -> MergeFavor {
    let mut favor = MergeFavor::None;
    for option in options {
        favor = match option.as_str() {
            "ours" => MergeFavor::Ours,
            "theirs" => MergeFavor::Theirs,
            "patience" | "histogram" | "minimal" | "ignore-space-change" | "ignore-all-space" | "ignore-space-at-eol" | "ignore-cr-at-eol" | "renormalize" | "no-renormalize" | "no-renames"
                => die(128, &format!("fatal: strategy option -X{} is not supported by rgit (use -X ours or -X theirs)", option)),
            o if o.starts_with("diff-algorithm=") || o.starts_with("find-renames") || o.starts_with("rename-threshold=") || o.starts_with("subtree")
                => die(128, &format!("fatal: strategy option -X{} is not supported by rgit (use -X ours or -X theirs)", option)),
            _ => die(128, &format!("fatal: unknown strategy option: -X{}", option)),
        };
    }
    favor
}

/// The merge commit's message: `-m` paragraphs or the `-F` file, else git's default, always
/// ending in one newline.
fn merge_commit_message(opts: &MergeOptions, name: &str, head_state: &refs::HeadState) -> anyhow::Result<String> {
    let text = if !opts.messages.is_empty() {
        opts.messages.join("\n\n")
    } else if let Some(file) = &opts.file {
        match fs::read_to_string(file) {
            Ok(text) => text,
            Err(_) => die(128, &format!("fatal: could not read file '{}'", file)),
        }
    } else {
        let current = match head_state {
            refs::HeadState::Branch(branch) => branch.as_str(),
            refs::HeadState::Detached(_) => "HEAD",
        };
        format!("{}{}", merge_message(name), crate::merge::fmt_merge_msg::engine::merge_into_suffix(current))
    };
    let cleaned = crate::core::commit::cleanup_commit_message(&text, false);
    Ok(if cleaned.ends_with('\n') { cleaned } else { format!("{}\n", cleaned) })
}

/// Rewords a switch/checkout refusal for a merge.
fn for_merge(error: &str) -> String {
    error.replace("by switch", "by merge").replace("before you switch branches", "before you merge")
}

/// Points the current branch (or a detached HEAD) at `commit`.
fn move_head(head_state: &refs::HeadState, commit: &str) -> anyhow::Result<()> {
    match head_state {
        refs::HeadState::Branch(branch) => refs::write_ref(&format!("refs/heads/{}", branch), commit),
        refs::HeadState::Detached(_) => refs::set_head_detached(commit),
    }
}

/// git's `squash_message`: `SQUASH_MSG` lists every commit being squashed (`HEAD..<theirs>`,
/// newest first) in the `medium` log format under "Squashed commit of the following:".
fn write_squash_message(ours: &str, theirs: &str) -> anyhow::Result<()> {
    use crate::history::rev_list::engine::{parse_rev_list_args, select_rev_list_commits};
    let rev_opts = parse_rev_list_args(&[theirs.to_string(), format!("^{}", ours)])?;
    let (walked, _) = select_rev_list_commits(&rev_opts)?;
    let log_opts = crate::history::log::types::LogOptions::default();
    let mut text = String::from("Squashed commit of the following:\n");
    for commit in walked.iter().filter(|c| !c.hidden) {
        text.push('\n');
        text.push_str(&crate::history::log::engine::format_log_entry(&commit.commit, &log_opts, &Default::default()));
    }
    fs::write(crate::core::repo::path(".git/SQUASH_MSG"), text)?;
    Ok(())
}

/// The `--stat --summary` block git prints after a merge or fast-forward (renames detected
/// per `diff.renames`).
fn print_diffstat(old: &TreeMap, new: &TreeMap) -> anyhow::Result<()> {
    use crate::diff::plumbing::{diff_trees, render_diff_changes};
    let opts = crate::diff::plumbing::types::DiffPlumbingOptions {
        recursive: true,
        stat: true,
        summary: true,
        renames: crate::diff::rename::default_rename_options("diff.renames"),
        ..Default::default()
    };
    let (old_tree, new_tree) = (build_tree_from_map(old)?, build_tree_from_map(new)?);
    let mut changes = Vec::new();
    diff_trees(Some(&old_tree), Some(&new_tree), "", &opts, &[], &mut changes)?;
    let changes = crate::diff::rename::detect_renames(changes, &opts.renames, &[]);
    print!("{}", render_diff_changes(&changes, &opts)?);
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
            crate::ui::exit(1);
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
                crate::ui::exit(1);
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
                None => crate::ui::exit(1),
            }
        }
        println!("{}", current_base);
    }

    Ok(())
}
