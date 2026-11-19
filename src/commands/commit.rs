use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use crate::helpers::commit::{
    add_signoff, cleanup_commit_message, current_author_signature, current_signature, edit_commit_message, parse_author_option, parse_commit_date,
    stage_tracked_changes, tracked_worktree_changes, write_commit_object,
};
use crate::helpers::format_patch::{parse_commit_object, resolve_commit_rev, split_commit_message};
use crate::helpers::reflog::set_reflog_action;
use crate::helpers::{build_commit, flatten_tree, write_tree_from_index_prefix};
use crate::index::read_index;
use crate::objects::CommitOptions;
use crate::refs;

pub fn commit_tree(tree_hash: String, parent_hash: Option<String>, message: String) -> anyhow::Result<()> {
    println!("{}", build_commit(tree_hash, parent_hash, &message)?);
    Ok(())
}

/// `rgit commit [-m <msg>]... [-F <file>] [-C|-c <commit>] [-a] [--amend] [-e|--no-edit]
/// [--author=<who>] [--date=<when>] [--reset-author] [--allow-empty] [--allow-empty-message]
/// [-s] [-q]`
///
/// Records the index as a new commit on the current branch (or detached HEAD). The message
/// comes from `-m` (several become paragraphs), `-F`, `-C`/`-c` (another commit's message and
/// author), or the editor, which opens when none is given (and with `-e`, `--amend`, or to
/// conclude a merge), prefilled and with `#` lines stripped. `-a` stages tracked changes first;
/// `--amend` replaces the tip, keeping its parents, message and author unless told otherwise.
/// Refuses empty commits and empty messages unless allowed, and refuses to commit while the
/// index has unmerged paths. Prints git's summary: `[branch hash] subject` plus a
/// diffstat line and create/delete modes.
pub fn commit(opts: CommitOptions) -> anyhow::Result<()> {
    let merging = crate::repo::path(".git/MERGE_HEAD").exists();
    if merging && opts.amend {
        fail(128, "fatal: You are in the middle of a merge -- cannot amend.");
    }
    // `-a` resolves conflicts by staging the working tree; otherwise unmerged paths block.
    if opts.all {
        stage_tracked_changes()?;
    }
    crate::helpers::merge::die_if_unmerged_for_commit()?;

    let head_state = refs::resolve_head()?;
    let head = refs::resolve_head_commit()?;
    let head_commit = match (&head, opts.amend) {
        (Some(h), _) => Some(parse_commit_object(h)?),
        (None, true) => fail(128, "fatal: You have nothing to amend."),
        (None, false) => None,
    };
    let parents: Vec<String> = if opts.amend {
        head_commit.as_ref().map(|c| c.parents.clone()).unwrap_or_default()
    } else {
        let mut parents: Vec<String> = head.iter().cloned().collect();
        if merging {
            parents.push(fs::read_to_string(crate::repo::path(".git/MERGE_HEAD"))?.trim().to_string());
        }
        parents
    };

    let entries = read_index().unwrap_or_default();
    let tree = write_tree_from_index_prefix(&entries, "")?;
    let base_tree = match parents.first() {
        Some(p) => Some(parse_commit_object(p)?.tree),
        None => None,
    };
    if !merging && !opts.allow_empty && base_tree.as_deref().unwrap_or(EMPTY_TREE) == tree {
        report_nothing_to_commit(&head_state, head.is_none(), opts.amend)?;
    }

    let committer = current_signature()?;
    let reused = match &opts.reuse_message {
        Some(rev) => Some(parse_commit_object(&resolve_commit_rev(rev)?)?),
        None => None,
    };
    // git takes the author from the commit being reused, amended or cherry-picked; its date
    // is then "interesting" and shown in the summary.
    let picked = if reused.is_none() && !opts.amend {
        fs::read_to_string(crate::repo::path(".git/CHERRY_PICK_HEAD")).ok().and_then(|h| parse_commit_object(h.trim()).ok())
    } else {
        None
    };
    let mut author = match (&reused, &head_commit, &picked) {
        (Some(c), _, _) if !opts.reset_author => c.author.clone(),
        (None, Some(c), _) if opts.amend && !opts.reset_author => c.author.clone(),
        (None, _, Some(c)) if !opts.reset_author => c.author.clone(),
        _ => current_author_signature()?,
    };
    let show_date = opts.date.is_some() || (!opts.reset_author && (reused.is_some() || (opts.amend && head_commit.is_some()) || picked.is_some()));
    if let Some(who) = &opts.author {
        let (name, email) = parse_author_option(who).unwrap_or_else(|e| fail(128, &e.to_string()));
        author.name = name;
        author.email = email;
    }
    if let Some(when) = &opts.date {
        let (timestamp, tz) = parse_commit_date(when).unwrap_or_else(|e| fail(128, &e.to_string()));
        author.timestamp = timestamp;
        author.tz = tz;
    }

    let given = if !opts.messages.is_empty() {
        Some(opts.messages.join("\n\n"))
    } else if let Some(file) = &opts.file {
        Some(if file == "-" {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text)?;
            text
        } else {
            fs::read_to_string(file).map_err(|_| anyhow::anyhow!("fatal: could not read log file '{}'", file))?
        })
    } else {
        reused.as_ref().map(|c| c.message.clone())
    };
    let template = match (&given, &head_commit) {
        (Some(text), _) => text.clone(),
        (None, Some(c)) if opts.amend => c.message.clone(),
        // git's order: MERGE_MSG (after SQUASH_MSG when a squash merge conflicted), else
        // SQUASH_MSG.
        _ => {
            let read = |name: &str| fs::read_to_string(crate::repo::path(format!(".git/{}", name))).ok();
            match (read("MERGE_MSG"), read("SQUASH_MSG")) {
                (Some(merge_msg), squash) => format!("{}{}", squash.unwrap_or_default(), merge_msg),
                (None, Some(squash)) => squash,
                (None, None) => String::new(),
            }
        }
    };
    let use_editor = !opts.no_edit && (opts.edit || opts.reedit || given.is_none());
    let mut message = if use_editor {
        cleanup_commit_message(&edit_commit_message(&template, &editor_status(&head_state, &tree, base_tree.as_deref())?)?, true)
    } else {
        // Without an editor git's cleanup is "whitespace": `#` lines stay (unless --cleanup=strip).
        cleanup_commit_message(&template, opts.cleanup_strip)
    };
    if opts.signoff {
        message = add_signoff(&message, &committer);
    }
    if message.trim().is_empty() && !opts.allow_empty_message {
        fail(1, "Aborting commit due to empty commit message.");
    }
    fs::write(crate::repo::path(".git/COMMIT_EDITMSG"), &message)?;

    let subject = split_commit_message(&message).0;
    set_reflog_action(&match () {
        _ if opts.amend => format!("commit (amend): {}", subject),
        _ if merging => format!("commit (merge): {}", subject),
        _ if parents.is_empty() => format!("commit (initial): {}", subject),
        _ => format!("commit: {}", subject),
    });
    let commit_hash = write_commit_object(&tree, &parents, &author, &committer, &message)?;
    match &head_state {
        refs::HeadState::Branch(branch) => refs::write_ref(&format!("refs/heads/{}", branch), &commit_hash)?,
        refs::HeadState::Detached(_) => refs::set_head_detached(&commit_hash)?,
    }
    crate::helpers::merge::clear_merge_state();

    crate::helpers::sequencer::post_commit_cleanup();

    if !opts.quiet {
        crate::helpers::commit::print_commit_summary(&head_state, &commit_hash, &author, &committer, show_date)?;
    }
    Ok(())
}

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

fn fail(code: i32, message: &str) -> ! {
    eprintln!("{}", message);
    crate::ui::exit(code);
}

/// Prints git's explanation of why there is nothing to commit and exits with status 1.
fn report_nothing_to_commit(head_state: &refs::HeadState, unborn: bool, amend: bool) -> anyhow::Result<()> {
    match head_state {
        refs::HeadState::Branch(branch) => println!("On branch {}", branch),
        refs::HeadState::Detached(hash) => println!("HEAD detached at {}", &hash[..7]),
    }
    if amend {
        println!("No changes");
        eprintln!("You asked to amend the most recent commit, but doing so would make\nit empty. You can repeat your command with --allow-empty, or you can\nremove the commit entirely with \"rgit reset HEAD^\".");
        crate::ui::exit(1);
    }
    let changes = tracked_worktree_changes()?;
    if unborn {
        println!("\nInitial commit\n\nnothing to commit (create/copy files and use \"rgit add\" to track)");
    } else if changes.is_empty() {
        println!("nothing to commit, working tree clean");
    } else {
        println!("Changes not staged for commit:\n  (use \"rgit add <file>...\" to update what will be committed)\n  (use \"rgit restore <file>...\" to discard changes in working directory)");
        for (kind, path) in changes {
            println!("\t{}   {}", if kind == 'D' { "deleted: " } else { "modified:" }, path);
        }
        println!("\nno changes added to commit (use \"rgit add\" and/or \"rgit commit -a\")");
    }
    crate::ui::exit(1);
}

/// The commented status block shown in the editor: the branch and what will be committed.
fn editor_status(head_state: &refs::HeadState, tree: &str, base_tree: Option<&str>) -> anyhow::Result<String> {
    let mut text = match head_state {
        refs::HeadState::Branch(branch) => format!("# On branch {}\n", branch),
        refs::HeadState::Detached(hash) => format!("# HEAD detached at {}\n", &hash[..7]),
    };
    let (mut old, mut new) = (BTreeMap::new(), BTreeMap::new());
    if let Some(base) = base_tree {
        flatten_tree(base, "", &mut old)?;
    }
    flatten_tree(tree, "", &mut new)?;
    let paths: std::collections::BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    let mut lines = Vec::new();
    for path in paths {
        match (old.get(path), new.get(path)) {
            (None, Some(_)) => lines.push(format!("#\tnew file:   {}", path)),
            (Some(_), None) => lines.push(format!("#\tdeleted:    {}", path)),
            (Some(a), Some(b)) if a != b => lines.push(format!("#\tmodified:   {}", path)),
            _ => {}
        }
    }
    if !lines.is_empty() {
        text.push_str("# Changes to be committed:\n");
        text.push_str(&lines.join("\n"));
        text.push('\n');
    }
    text.push_str("#\n");
    Ok(text)
}
