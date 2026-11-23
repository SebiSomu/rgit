pub mod cli;
pub mod engine;
pub mod types;

use crate::core::commit::{current_signature, is_reachable, parse_commit_object, write_commit_object};
use crate::core::refs;
use crate::merge::split::engine::{ask, commits_above, pieces_of, show_piece, tree_with};
use crate::merge::split::types::{Piece, SplitSource};

/// `rgit split [<commit>] [-f]`: splits one commit into several by hunk. The commit's changes
/// are cut into pieces (text hunks; whole-file changes for new, deleted and binary files and
/// mode changes) and the user picks, commit by commit, which pieces go into it and its
/// message; what is left goes on to the next one. The last commit ends at the original tree,
/// so the commits on top keep their trees and are only re-parented: the working tree and the
/// index are never touched. Merge commits, the root commit and (unless `--force`) commits a
/// remote-tracking branch has are refused.
pub fn split(commit: Option<String>, force: bool) -> anyhow::Result<()> {
    crate::workspace::fixup::refuse_if_busy();
    let head_state = refs::resolve_head()?;
    let Some(head) = refs::resolve_head_commit()? else {
        crate::ui::die_fatal("there are no commits to split yet");
    };
    let rev = commit.unwrap_or_else(|| "HEAD".to_string());
    let target = match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", rev)) {
        Ok(hash) => hash,
        Err(_) => crate::ui::die_fatal(format!("'{}' is not a commit", rev)),
    };
    let short = target[..7].to_string();
    if target != head && !is_reachable(&head, &target)? {
        crate::ui::die_fatal(format!("{} is not in the current branch's history", short));
    }
    let original = parse_commit_object(&target)?;
    let subject = original.message.lines().next().unwrap_or("").to_string();
    let parent = match original.parents.as_slice() {
        [parent] => parent.clone(),
        [] => crate::ui::die_fatal(format!("{} is the root commit; rgit can't split it", short)),
        _ => crate::ui::die_fatal(format!("{} is a merge commit; rgit can't split merges", short)),
    };
    let above = match commits_above(&head, &target) {
        Ok(list) => list,
        Err(e) => crate::ui::die_fatal(format!("can't rewrite the commits on top of {}: {}", short, e)),
    };
    if !force && let Some(remote) = crate::workspace::fixup::engine::pushed_on(&target)? {
        crate::ui::die_fatal(format!(
            "{} is already pushed ({} has it); splitting it would make the branches diverge\nhint: pass --force to split it anyway (you'll need `rgit push --force` afterwards)",
            short, remote
        ));
    }
    let source = pieces_of(&target, &parent)?;
    let total = source.pieces.len();
    if total < 2 {
        crate::ui::die_fatal(format!("{} {} has a single hunk; there is nothing to split", short, subject));
    }

    // Pick the pieces of each new commit, in order.
    println!("Splitting {} {} ({} hunks).", short, subject, total);
    println!("Pick the hunks of each new commit; the ones you leave go on to the next commit.");
    let mut remaining: Vec<usize> = (0..total).collect();
    let mut groups: Vec<(Vec<usize>, String)> = Vec::new();
    while !remaining.is_empty() {
        let k = groups.len() + 1;
        let chosen = if remaining.len() == 1 {
            println!("\nThe last hunk goes into commit {}.", k);
            remaining.clone()
        } else {
            match pick_round(&source, &remaining, k)? {
                Some(chosen) => chosen,
                None => return cancelled(),
            }
        };
        let last = chosen.len() == remaining.len();
        let default = if last { original.message.clone() } else { format!("{} (part {})\n", subject, k) };
        let first_line = default.lines().next().unwrap_or("");
        let Some(message) = ask(&format!("Message for commit {} (Enter: \"{}\"): ", k, first_line))? else {
            return cancelled();
        };
        let message = if message.is_empty() { default } else { format!("{}\n", message) };
        remaining.retain(|i| !chosen.contains(i));
        groups.push((chosen, message));
    }

    // The new commits, each adding its pieces on top of the previous one.
    let committer = current_signature()?;
    let mut tip = parent;
    let mut taken: Vec<&Piece> = Vec::new();
    let mut made: Vec<(String, String)> = Vec::new();
    for (chosen, message) in &groups {
        taken.extend(chosen.iter().map(|&i| &source.pieces[i]));
        let tree = tree_with(&source, &taken)?;
        tip = write_commit_object(&tree, std::slice::from_ref(&tip), &original.author, &committer, message)?;
        made.push((tip.clone(), message.lines().next().unwrap_or("").to_string()));
    }
    if crate::core::object_store::tree_hash_of_commit(&tip)? != original.tree {
        anyhow::bail!("fatal: the split commits don't add up to {} (nothing was changed)", short);
    }
    // Same trees on top, new parents.
    for hash in &above {
        let c = parse_commit_object(hash)?;
        tip = write_commit_object(&c.tree, std::slice::from_ref(&tip), &c.author, &committer, &c.message)?;
    }
    match head_state {
        refs::HeadState::Branch(branch) => refs::write_ref(&format!("refs/heads/{}", branch), &tip)?,
        refs::HeadState::Detached(_) => refs::set_head_detached(&tip)?,
    }

    println!("\nSplit {} into {} commits:", short, made.len());
    for (hash, first) in &made {
        println!("  {} {}", &hash[..7], first);
    }
    if !above.is_empty() {
        let which = if above.len() == 1 { "the commit".to_string() } else { format!("the {} commits", above.len()) };
        println!("Rewrote {} on top; the files are unchanged.", which);
    }
    Ok(())
}

/// One round of the picker: asks about every remaining piece whether it goes into commit `k`.
/// Repeats until at least one is taken (and, for the first commit, not all of them). `None`
/// when the user quits or the input ends.
// Used by `split`.
fn pick_round(source: &SplitSource, remaining: &[usize], k: usize) -> anyhow::Result<Option<Vec<usize>>> {
    loop {
        println!("\nCommit {}: y = take the hunk, n = leave it, a = take it and the rest, d = leave it and the rest, q = quit (nothing changes)", k);
        let mut chosen = Vec::new();
        let mut rest: Option<bool> = None;
        for &i in remaining {
            if let Some(take) = rest {
                if take {
                    chosen.push(i);
                }
                continue;
            }
            println!();
            show_piece(source, &source.pieces[i]);
            loop {
                let Some(answer) = ask(&format!("Into commit {} [y,n,a,d,q]? ", k))? else {
                    return Ok(None);
                };
                match answer.chars().next().map(|c| c.to_ascii_lowercase()) {
                    Some('y') => chosen.push(i),
                    Some('n') => {}
                    Some('a') => {
                        chosen.push(i);
                        rest = Some(true);
                    }
                    Some('d') => rest = Some(false),
                    Some('q') => return Ok(None),
                    _ => {
                        println!("y = take the hunk, n = leave it, a = take it and the rest, d = leave it and the rest, q = quit");
                        continue;
                    }
                }
                break;
            }
        }
        if chosen.is_empty() {
            println!("\nCommit {} needs at least one hunk; once more.", k);
        } else if k == 1 && chosen.len() == remaining.len() {
            println!("\nThat's every hunk, the commit would stay as it is: leave at least one for a later commit; once more.");
        } else {
            return Ok(Some(chosen));
        }
    }
}

/// Prints that nothing changed.
// Used by `split` when the user quits.
fn cancelled() -> anyhow::Result<()> {
    println!("Cancelled; nothing changed.");
    Ok(())
}
