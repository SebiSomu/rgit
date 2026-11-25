//! Helpers of `rgit whereami`: reading the repository's situation, describing the operation in
//! progress and choosing the likely next command.

use anyhow::Result;

use crate::core::config::get_config_value;
use crate::core::refs;
use crate::workspace::whereami::types::{Situation, Upstream};

/// Reads the branch, HEAD, upstream, uncommitted changes, operation in progress, stash and
/// `rgit wip` saves.
// Used by the `whereami` command.
pub fn gather() -> Result<Situation> {
    let snap = crate::ui::narrate::take_snapshot(true);
    let branch = snap.head_branch.clone();
    let worktree = snap.worktree_changes.unwrap_or_default();
    let operation = snap.operation.clone().or_else(|| crate::core::repo::path(".git/rebase-apply").exists().then(|| "am".to_string()));
    let wip_count = match &branch {
        Some(b) => crate::workspace::wip::engine::read_stack(b).map(|s| s.len()).unwrap_or(0),
        None => 0,
    };
    Ok(Situation {
        upstream: match &branch {
            Some(b) => upstream(b)?,
            None => Upstream::None(None),
        },
        branch,
        head: snap.head,
        staged: snap.staged.len(),
        modified: worktree.iter().filter(|(_, p)| !snap.unmerged.contains_key(p)).count(),
        untracked: snap.untracked.map(|u| u.len()).unwrap_or(0),
        conflicted: snap.unmerged.len(),
        operation,
        stash_count: snap.stash_count,
        wip_count,
    })
}

/// The branch's upstream and the ahead/behind counts against its remote-tracking ref.
// Used by `gather`.
fn upstream(branch: &str) -> Result<Upstream> {
    let get = |k: &str| get_config_value(&format!("branch.{}.{}", branch, k));
    let (Some(remote), Some(merge)) = (get("remote")?, get("merge")?) else {
        let remotes = crate::remote::remote::engine::list_remote_names().unwrap_or_default();
        let target = remotes.iter().find(|r| *r == "origin").or(remotes.first()).cloned();
        return Ok(Upstream::None(target));
    };
    let short = merge.strip_prefix("refs/heads/").unwrap_or(&merge).to_string();
    let (name, refname) = if remote == "." { (short.clone(), format!("refs/heads/{}", short)) } else { (format!("{}/{}", remote, short), format!("refs/remotes/{}/{}", remote, short)) };
    let Some(tip) = refs::read_ref(&refname)? else {
        return Ok(Upstream::Gone(name));
    };
    let head = refs::read_ref(&format!("refs/heads/{}", branch))?.unwrap_or_default();
    let ours = crate::core::commit::collect_ancestors(&head)?;
    let theirs = crate::core::commit::collect_ancestors(&tip)?;
    Ok(Upstream::Tracking { name, ahead: ours.difference(&theirs).count(), behind: theirs.difference(&ours).count() })
}

/// One line on the operation in progress: what it is and how far it got.
// Used by the `whereami` command.
pub fn describe_operation(op: &str) -> String {
    use crate::merge::rebase::engine as rebase;
    let read = |p: &str| std::fs::read_to_string(crate::core::repo::path(p)).ok().map(|s| s.trim().to_string());
    let short = |h: &str| h.get(..7).unwrap_or(h).to_string();
    let subject = |h: &str| crate::ui::narrate::commit_info(h).map(|(s, _, _)| s).unwrap_or_default();
    match op {
        "rebase" => {
            let branch = read(rebase::REBASE_BRANCH_PATH).filter(|b| !b.is_empty()).map(|b| format!(" of {}", b.strip_prefix("refs/heads/").unwrap_or(&b))).unwrap_or_default();
            let onto = format!("{}{}", branch, read(rebase::REBASE_ONTO_PATH).map(|h| format!(" onto {}", short(&h))).unwrap_or_default());
            let left = rebase::read_todo().map(|t| t.len()).unwrap_or(0);
            match read(rebase::REBASE_TOTAL_PATH).and_then(|t| t.parse::<usize>().ok()) {
                Some(total) if total >= left => format!("rebase{}: {} of {} commits done", onto, total - left, total),
                _ => format!("rebase{}: {} commits left", onto, left),
            }
        }
        "merge" => match read(".git/MERGE_HEAD") {
            Some(h) => format!("merge of {} {}", short(&h), subject(&h)),
            None => "merge".to_string(),
        },
        "cherry-pick" | "revert" => match read(&format!(".git/{}_HEAD", op.to_uppercase().replace('-', "_"))) {
            Some(h) => format!("{} of {} {}", op, short(&h), subject(&h)),
            None => op.to_string(),
        },
        "bisect" => match refs::resolve_head_commit().ok().flatten() {
            Some(h) => format!("bisect, testing {} {}", short(&h), subject(&h)),
            None => "bisect".to_string(),
        },
        "am" => "am (applying patches from a mailbox)".to_string(),
        "stash" => "stash apply that stopped on conflicts".to_string(),
        other => other.to_string(),
    }
}

/// The branch a rebase or bisect started from, while HEAD is detached for it.
// Used by the `whereami` command's header line.
pub fn detached_for(op: Option<&str>) -> Option<String> {
    let read = |p: &str| std::fs::read_to_string(crate::core::repo::path(p)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let branch = |b: String| b.trim_start_matches("ref: ").trim_start_matches("refs/heads/").to_string();
    match op? {
        "rebase" => read(crate::merge::rebase::engine::REBASE_BRANCH_PATH).map(|b| format!("rebasing {}", branch(b))),
        "bisect" => read(crate::history::bisect::engine::BISECT_START_PATH).filter(|b| b.len() != 40).map(|b| format!("bisecting, started on {}", branch(b))),
        _ => None,
    }
}

/// The command most likely wanted next, as a sentence with the commands in backticks.
// Used by the `whereami` command.
pub fn next_step(s: &Situation) -> String {
    if let Some(op) = s.operation.as_deref() {
        let finish = match op {
            "merge" | "rebase" | "cherry-pick" | "revert" | "am" => format!("`rgit {} --continue`", op),
            "stash" => "`rgit stash drop` once you're done with the stash entry".to_string(),
            _ => String::new(),
        };
        if s.conflicted > 0 {
            return format!("resolve the conflicts (`rgit conflicts` shows them), `rgit add` each file, then {}", finish);
        }
        return match op {
            "bisect" => "test this commit, then `rgit bisect good` or `rgit bisect bad` (`rgit bisect reset` ends it)".to_string(),
            "stash" => format!("conflicts resolved: {}", finish),
            _ => format!("{} (or `rgit {} --abort` to cancel it)", finish, op),
        };
    }
    if s.conflicted > 0 {
        return "resolve the conflicts (`rgit conflicts` shows them) and `rgit add` each file".to_string();
    }
    let Some(branch) = s.branch.as_deref() else {
        return "`rgit switch -c <name>` to keep commits made here on a branch, or `rgit switch <branch>` to go back".to_string();
    };
    if s.head.is_none() {
        return "`rgit add` your files, then `rgit commit` (the first commit)".to_string();
    }
    if s.staged > 0 {
        return if s.modified > 0 { "`rgit commit` the staged changes (`rgit add` the rest first, or `rgit commit -a` for everything)".to_string() } else { "`rgit commit`".to_string() };
    }
    if s.modified > 0 {
        return "`rgit add <file>` and `rgit commit` (or `rgit commit -a` for every modified file)".to_string();
    }
    match &s.upstream {
        Upstream::Tracking { name, ahead, behind } if *behind > 0 => format!(
            "`rgit sync` to bring in {}'s new {}{}",
            name,
            if *behind == 1 { "commit" } else { "commits" },
            if *ahead > 0 { " and replay yours on top" } else { "" }
        ),
        Upstream::Tracking { ahead, .. } if *ahead > 0 => "`rgit push`".to_string(),
        Upstream::None(Some(remote)) => format!("`rgit push -u {} {}` to publish the branch (or `rgit sync --push`)", remote, branch),
        Upstream::Gone(name) => format!("{} was deleted on the remote: `rgit branch -d {}` if it's merged, or `rgit sync --push` to publish it again", name, branch),
        _ if s.untracked > 0 => "`rgit add <file>` to track the new files (or list them in .gitignore)".to_string(),
        Upstream::Tracking { .. } => "nothing to do: everything is committed and level with the last fetch (`rgit fetch` to check for new commits)".to_string(),
        Upstream::None(None) => "nothing to do: everything is committed".to_string(),
    }
}
