pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;

use crate::workspace::recover::engine::{find_lost_work, format_when};
use crate::workspace::recover::types::{LostKind, LostWork};

/// `rgit recover`: lists lost work, newest first: commits a reset, amend, rebase or deleted
/// branch left behind (from any reflog), dropped stashes and `wip` saves, and commits nothing
/// points at any more. Each gets a number for `rgit recover <n>`.
// Used by `rgit recover`.
pub fn list() -> Result<()> {
    let lost = find_lost_work()?;
    if lost.is_empty() {
        println!("No lost work: every commit in the repository is on a branch, a tag, the stash or HEAD.");
        return Ok(());
    }
    println!("Lost work, newest first:");
    for (i, work) in lost.iter().enumerate() {
        println!("  [{}] {}  {}  {}", i + 1, short(&work.commit), format_when(work.time, &work.tz), work.subject);
        println!("      {}", describe(work));
    }
    println!("`rgit show <commit>` to look at one; `rgit recover <n> [<branch>]` brings it back on a branch");
    println!("(`recovered-<commit>` by default), and a stash back on the stash list.");
    Ok(())
}

/// `rgit recover <n|commit> [<branch>]`: brings one piece of lost work back. A commit gets a
/// branch (`<branch>`, or `recovered-<short hash>`); a dropped stash goes back on the stash
/// list as `stash@{0}`, or onto a branch when `<branch>` is given.
// Used by `rgit recover <n>`.
pub fn restore(pick: &str, branch: Option<&str>) -> Result<()> {
    let lost = find_lost_work()?;
    let work = choose(&lost, pick)?;
    if work.kind == LostKind::Stash && branch.is_none() {
        crate::workspace::state_files::append_stash_entry(&work.commit, &work.subject)?;
        println!("Restored stash@{{0}}: {}", work.subject);
        println!("`rgit stash show -p` shows it, `rgit stash pop` applies it.");
        return Ok(());
    }
    let name = branch.map(str::to_string).unwrap_or_else(|| format!("recovered-{}", short(&work.commit)));
    crate::core::refs::create_branch(&name, &work.commit)?;
    crate::core::reflog::record_ref_change(&format!("refs/heads/{}", name), None, Some(&work.commit), "branch: recovered by rgit recover", true);
    println!("Created branch '{}' at {} ({}): {} back.", name, short(&work.commit), work.subject, plural(work.commits, "commit", "commits"));
    println!("`rgit switch {}` to work on it, or `rgit merge {}` / `rgit cherry-pick {}` to bring it into your branch.", name, name, short(&work.commit));
    Ok(())
}

/// The listed piece `pick` names: its number in the listing, or (a prefix of) its commit.
// Used by `restore`.
fn choose<'a>(lost: &'a [LostWork], pick: &str) -> Result<&'a LostWork> {
    if lost.is_empty() {
        anyhow::bail!("fatal: there is no lost work to recover (`rgit recover` lists it)");
    }
    if let Ok(n) = pick.parse::<usize>()
        && pick.len() < 4
    {
        return match n.checked_sub(1).and_then(|i| lost.get(i)) {
            Some(work) => Ok(work),
            None => anyhow::bail!("fatal: there is no lost work number {} (`rgit recover` lists {})", n, lost.len()),
        };
    }
    let pick = pick.to_ascii_lowercase();
    let matches: Vec<&LostWork> = lost.iter().filter(|w| pick.len() >= 4 && w.commit.starts_with(&pick)).collect();
    match matches.as_slice() {
        [work] => Ok(work),
        [] => anyhow::bail!("fatal: '{}' is not in the list of lost work (`rgit recover` lists it by number)", pick),
        _ => anyhow::bail!("fatal: '{}' matches several pieces of lost work; use more digits or the number", pick),
    }
}

/// The second line of a listing entry: how much comes back and how it was lost.
// Used by `list`.
fn describe(work: &LostWork) -> String {
    match &work.kind {
        LostKind::Stash => "stash that was dropped or popped".to_string(),
        LostKind::LeftBehind { entry } => format!("{}, left behind by {}", plural(work.commits, "commit", "commits"), entry),
        LostKind::Dangling => format!("{}, not on any branch and in no reflog", plural(work.commits, "commit", "commits")),
    }
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}
