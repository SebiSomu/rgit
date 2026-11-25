pub mod cli;
pub mod engine;
pub mod types;

use crate::core::types::ParsedCommit;
use crate::history::compare::engine::{copies, file_differences, merge_conflicts, only_on};
use crate::history::compare::types::Change;

/// `rgit compare [<a> [<b>]] [-n <N>]`: where two branches or commits stand against each
/// other, in one answer: the same commit, ahead, behind, diverged (since which merge base) or
/// unrelated; the commits only each side has (and how many of them the other side has as a
/// copy); the files that differ and how each side changed them; and, when they diverged,
/// whether merging them would conflict (worked out in memory) with the command to do it.
/// One argument compares HEAD with it; none, HEAD with its upstream.
pub fn compare(revs: Vec<String>, limit: usize) -> anyhow::Result<()> {
    let head_name = || match crate::core::refs::resolve_head() {
        Ok(crate::core::refs::HeadState::Branch(b)) => b,
        _ => "HEAD".to_string(),
    };
    let (left_rev, left_name, right_name) = match revs.as_slice() {
        [] => match crate::history::cherry::engine::resolve_cherry_upstream()? {
            Some(up) => ("HEAD".to_string(), head_name(), up.trim_start_matches("refs/remotes/").trim_start_matches("refs/heads/").to_string()),
            None => crate::ui::die_fatal("the current branch has no upstream to compare with\nhint: name what to compare: `rgit compare <branch>` (HEAD against it) or `rgit compare <a> <b>`"),
        },
        [b] => ("HEAD".to_string(), head_name(), b.clone()),
        [a, b] => (a.clone(), a.clone(), b.clone()),
        _ => unreachable!("clap allows at most two"),
    };
    let resolve = |rev: &str| match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", rev)) {
        Ok(hash) => hash,
        Err(_) => crate::ui::die_fatal(format!("'{}' is not a branch or commit", rev)),
    };
    let (left, right) = (resolve(&left_rev), resolve(&right_name));
    let compares_head = revs.len() < 2 || left_name == head_name();
    let upstream = revs.is_empty();
    let (l, r) = (left_name.as_str(), right_name.as_str());
    let subject = |h: &str| crate::ui::narrate::commit_info(h).map(|(s, _, _)| s).unwrap_or_default();
    let commits = |n: usize| crate::remote::sync::engine::commits(n);

    if left == right {
        println!("{} and {} point to the same commit, {} \"{}\": nothing differs.", l, r, &left[..7], subject(&left));
        return Ok(());
    }
    let base = crate::merge::merge::engine::find_merge_base(&left, &right)?;
    let (mine, theirs) = (only_on(&left, &right)?, only_on(&right, &left)?);
    let since = |b: &str| format!("{} \"{}\" ({})", &b[..7], subject(b), crate::history::story::engine::day(crate::core::commit::parse_commit_object(b).map(|c| c.author.timestamp).unwrap_or(0)));
    match (&base, mine.len(), theirs.len()) {
        (None, _, _) => println!("{} and {} share no history: {} and {} commits of their own.", l, r, mine.len(), theirs.len()),
        (Some(_), a, 0) => println!("{} is {} ahead of {} ({} can fast-forward to it).", l, commits(a), r, r),
        (Some(_), 0, b) => println!("{} is {} behind {} (it can fast-forward to {}).", l, commits(b), r, r),
        (Some(b), _, _) => {
            println!("{} and {} have diverged since {}:", l, r, since(b));
            for (name, other, list, against) in [(l, r, &mine, &theirs), (r, l, &theirs, &mine)] {
                let copied = copies(list, against)?;
                let note = if copied > 0 { format!(" ({} of them {} too, as a copy)", copied, if copied == 1 { format!("is on {}", other) } else { format!("are on {}", other) }) } else { String::new() };
                println!("  {:<w$}  {} {} doesn't have{}", format!("{}:", name), commits(list.len()), other, note, w = l.len().max(r.len()) + 1);
            }
        }
    }

    for (name, list) in [(l, &mine), (r, &theirs)] {
        if list.is_empty() {
            continue;
        }
        println!();
        println!("Only on {}", name);
        print_commits(list, limit);
    }

    let files = file_differences(&left, &right, base.as_deref())?;
    if !files.is_empty() {
        println!();
        println!("{} differ{}", if files.len() == 1 { "1 file".to_string() } else { format!("{} files", files.len()) }, if files.len() == 1 { "s" } else { "" });
        let width = files.iter().take(limit * 2).map(|f| f.path.chars().count()).max().unwrap_or(0);
        for f in files.iter().take(limit * 2) {
            let how = match (f.left, f.right) {
                (Change::Same, c) => format!("{} on {}", c.verb(), r),
                (c, Change::Same) => format!("{} on {}", c.verb(), l),
                (a, b) if a == b => format!("{} on both sides", a.verb()),
                (a, b) => format!("{} on {}, {} on {}", a.verb(), l, b.verb(), r),
            };
            println!("  {:<w$}  {}", f.path, how, w = width);
        }
        if files.len() > limit * 2 {
            println!("  and {} more", files.len() - limit * 2);
        }
    }

    println!();
    match (&base, mine.len(), theirs.len()) {
        (None, _, _) => println!("Merging them needs `--allow-unrelated-histories`."),
        (Some(_), _, 0) => println!("Next: {}", if upstream { "`rgit push`.".to_string() } else { format!("`rgit merge {}` on {} fast-forwards it.", l, r) }),
        (Some(_), 0, _) => println!("Next: {}", if upstream { "`rgit sync` fast-forwards it.".to_string() } else if compares_head { format!("`rgit merge {}` fast-forwards {}.", r, l) } else { format!("`rgit merge {}` on {} fast-forwards it.", r, l) }),
        (Some(_), _, _) => {
            let conflicts = merge_conflicts(&left, &right)?;
            let both = files.iter().filter(|f| f.left != Change::Same && f.right != Change::Same).count();
            if conflicts.is_empty() {
                println!("Merging would be clean{}.", if both > 0 { format!(" ({} changed on both sides, but in different places)", if both == 1 { "1 file".to_string() } else { format!("{} files", both) }) } else { String::new() });
            } else {
                println!("Merging would conflict in {}: {}.", if conflicts.len() == 1 { "1 file".to_string() } else { format!("{} files", conflicts.len()) }, conflicts.join(", "));
            }
            if upstream {
                println!("Next: `rgit sync` (rebases your commits onto it; `--merge` merges instead).");
            } else if compares_head {
                println!("Next: `rgit merge {}` or `rgit rebase {}`.", r, r);
            }
        }
    }
    Ok(())
}

/// `  a1b2c3d 2023-11-10 Fix parser (Ana)` for the first `limit` commits, then `and N more`.
// Used by `compare`.
fn print_commits(list: &[ParsedCommit], limit: usize) {
    for c in list.iter().take(limit) {
        let merge = if c.parents.len() > 1 { "  (merge)" } else { "" };
        println!("  {} {} {} ({}){}", &c.hash[..7], crate::history::story::engine::day(c.author.timestamp), c.message.lines().next().unwrap_or(""), c.author.name, merge);
    }
    if list.len() > limit {
        println!("  and {} more", list.len() - limit);
    }
}
