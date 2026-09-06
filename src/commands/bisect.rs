// ============================================================================
// bisect
// ============================================================================
//
// Simplified `git bisect`: narrows down the first commit reachable from a
// known-bad commit but not reachable from any known-good commit, by
// repeatedly checking out a midpoint and letting the caller mark it `good`,
// `bad`, or `skip`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use anyhow::Context;

use crate::helpers::bisect::{
    checkout_bisect_commit, mark_bad, mark_good, mark_skip, report_outcome,
};
use crate::helpers::state_files::{append_line, read_lines_set};
use crate::helpers::checkout::{check_switch_safety, ensure_working_tree_clean, sync_working_tree, update_index_from_tree};
use crate::helpers::commit::resolve_commit_from_source;
use crate::helpers::objects::{flatten_tree, tree_hash_of_commit};

use crate::index::read_index;
use crate::objects::{BisectAction, BisectOutcome};
use crate::refs;

pub const BISECT_START_PATH: &str = ".git/BISECT_START";
pub const BISECT_LOG_PATH: &str = ".git/BISECT_LOG";
pub const BISECT_BAD_PATH: &str = ".git/BISECT_BAD";
pub const BISECT_GOOD_PATH: &str = ".git/BISECT_GOOD";
pub const BISECT_SKIP_PATH: &str = ".git/BISECT_SKIP";

/// `rgit bisect start [<bad> [<good>...]]`
pub fn bisect_start(bad: Option<String>, good: Vec<String>) -> anyhow::Result<()> {
    if Path::new(BISECT_START_PATH).exists() {
        anyhow::bail!(
            "fatal: a bisect session is already in progress\n\
             hint: use 'rgit bisect reset' to start over"
        );
    }

    refs::resolve_head_commit()?
        .ok_or_else(|| anyhow::anyhow!("fatal: bad HEAD - I need a HEAD commit to bisect"))?;
    ensure_working_tree_clean("start a bisect")?;

    let head_content = fs::read_to_string(".git/HEAD").context("Failed to read .git/HEAD")?;
    fs::write(BISECT_START_PATH, &head_content).context("Failed to write .git/BISECT_START")?;

    let _ = fs::remove_file(BISECT_BAD_PATH);
    let _ = fs::remove_file(BISECT_GOOD_PATH);
    let _ = fs::remove_file(BISECT_SKIP_PATH);
    let _ = fs::remove_file(BISECT_LOG_PATH);
    append_line(BISECT_LOG_PATH, "git bisect start")?;

    let mut last_outcome = None;
    if let Some(bad_rev) = bad {
        last_outcome = Some(mark_bad(Some(bad_rev))?);
    }
    for good_rev in good {
        last_outcome = Some(mark_good(Some(good_rev))?);
    }

    if let Some(outcome) = last_outcome {
        report_outcome(&outcome)?;
    }

    Ok(())
}

/// `rgit bisect bad [<rev>]`
pub fn bisect_bad(rev: Option<String>) -> anyhow::Result<()> {
    let outcome = mark_bad(rev)?;
    report_outcome(&outcome)
}

/// `rgit bisect good [<rev>]`
pub fn bisect_good(rev: Option<String>) -> anyhow::Result<()> {
    let outcome = mark_good(rev)?;
    report_outcome(&outcome)
}

/// `rgit bisect skip [<rev>...]`
pub fn bisect_skip(revs: Vec<String>) -> anyhow::Result<()> {
    let outcome = mark_skip(revs)?;
    report_outcome(&outcome)
}

/// `rgit bisect reset [<commit>]`
pub fn bisect_reset(commit: Option<String>) -> anyhow::Result<()> {
    if !Path::new(BISECT_START_PATH).exists() {
        anyhow::bail!("fatal: We are not bisecting.");
    }

    if let Some(target) = commit {
        let hash = resolve_commit_from_source(&target)?;
        checkout_bisect_commit(&hash)?;
        println!("HEAD is now at {} (detached)", &hash[..hash.len().min(7)]);
    } else {
        let saved = fs::read_to_string(BISECT_START_PATH).context("Failed to read .git/BISECT_START")?;
        let trimmed = saved.trim();

        if let Some(branch_ref) = trimmed.strip_prefix("ref: ") {
            let branch_name = branch_ref.strip_prefix("refs/heads/").unwrap_or(branch_ref).to_string();
            let target_hash = refs::read_ref(branch_ref)?.ok_or_else(|| {
                anyhow::anyhow!("fatal: '{}' no longer exists; cannot restore original branch", branch_name)
            })?;

            let target_tree_hash = tree_hash_of_commit(&target_hash)?;
            let mut target_tree = BTreeMap::new();
            flatten_tree(&target_tree_hash, "", &mut target_tree)?;

            let mut head_tree = BTreeMap::new();
            if let Some(current_commit) = refs::resolve_head_commit()? {
                let current_tree_hash = tree_hash_of_commit(&current_commit)?;
                flatten_tree(&current_tree_hash, "", &mut head_tree)?;
            }

            let index_entries = read_index().unwrap_or_default();
            let index_map: BTreeMap<String, [u8; 20]> = index_entries.iter().map(|e| (e.path.clone(), e.hash)).collect();

            check_switch_safety(&target_tree, &head_tree, &index_map)?;
            sync_working_tree(&target_tree, &head_tree, &index_map)?;
            update_index_from_tree(&target_tree, &head_tree)?;

            refs::set_head(&branch_name)?;
            println!("Switched to branch '{}'", branch_name);
        } else {
            checkout_bisect_commit(trimmed)?;
            println!("HEAD is now at {} (detached)", &trimmed[..trimmed.len().min(7)]);
        }
    }

    let _ = fs::remove_file(BISECT_START_PATH);
    let _ = fs::remove_file(BISECT_LOG_PATH);
    let _ = fs::remove_file(BISECT_BAD_PATH);
    let _ = fs::remove_file(BISECT_GOOD_PATH);
    let _ = fs::remove_file(BISECT_SKIP_PATH);

    println!("Bisect session ended.");

    Ok(())
}

/// `rgit bisect log`
pub fn bisect_log() -> anyhow::Result<()> {
    if !Path::new(BISECT_LOG_PATH).exists() {
        anyhow::bail!("fatal: We are not bisecting.");
    }
    let content = fs::read_to_string(BISECT_LOG_PATH).context("Failed to read .git/BISECT_LOG")?;
    print!("{}", content);
    Ok(())
}

/// `rgit bisect run <cmd> [<args>...]`
///
/// Repeatedly runs the given command against each midpoint: exit code 0
/// marks it good, 125 marks it skipped (matching git's convention for
/// "untestable"), any other exit code in 1..=127 marks it bad. Stops once a
/// single first-bad commit is found.
pub fn bisect_run(command: Vec<String>) -> anyhow::Result<()> {
    if command.is_empty() {
        anyhow::bail!("fatal: 'rgit bisect run' requires a command to run");
    }
    if !Path::new(BISECT_START_PATH).exists() {
        anyhow::bail!("fatal: You need to start by \"rgit bisect start\"");
    }
    if !Path::new(BISECT_BAD_PATH).exists() || read_lines_set(BISECT_GOOD_PATH)?.is_empty() {
        anyhow::bail!("fatal: bisect run requires both a bad and at least one good commit to be marked first");
    }

    loop {
        println!("running {}", command.join(" "));
        let status = std::process::Command::new(&command[0])
            .args(&command[1..])
            .status()
            .with_context(|| format!("failed to run '{}'", command[0]))?;

        let code = status.code().unwrap_or(-1);

        let outcome = if code == 0 {
            mark_good(None)?
        } else if code == 125 {
            mark_skip(Vec::new())?
        } else if (1..=127).contains(&code) {
            mark_bad(None)?
        } else {
            anyhow::bail!("fatal: bisect run failed: exit code {} from '{}'", code, command[0]);
        };

        report_outcome(&outcome)?;

        match outcome {
            BisectOutcome::Found(_) => {
                println!("bisect run success");
                break;
            }
            BisectOutcome::WaitingForBad | BisectOutcome::WaitingForGood => {
                anyhow::bail!("fatal: bisect run cannot proceed further; missing bad/good commit");
            }
            BisectOutcome::Continue(_, _) => continue,
        }
    }

    Ok(())
}

/// Top-level dispatcher for `rgit bisect <action>`.
pub fn bisect(action: BisectAction) -> anyhow::Result<()> {
    match action {
        BisectAction::Start { bad, good } => bisect_start(bad, good),
        BisectAction::Bad { rev } => bisect_bad(rev),
        BisectAction::Good { rev } => bisect_good(rev),
        BisectAction::Skip { revs } => bisect_skip(revs),
        BisectAction::Reset { commit } => bisect_reset(commit),
        BisectAction::Log => bisect_log(),
        BisectAction::Run { command } => bisect_run(command),
    }
}