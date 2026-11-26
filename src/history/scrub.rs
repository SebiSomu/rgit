pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;

use crate::history::scrub::engine::{apply_scrub, backup_refs, drop_backup, normalize_paths, plan_scrub, BACKUP_PREFIX};
use crate::history::scrub::types::ScrubPlan;

/// `rgit scrub <path>...`: removes files or folders (a leaked secret, a big binary) from every
/// commit on the local branches and tags. The old branches and tags are kept under
/// `refs/rgit/scrub-backup/` and `rgit undo` takes the scrub back; with `dry_run` nothing is
/// written. Afterwards it says what still holds the old commits (reflogs, the backup, the
/// remote) and how to finish.
// Used by `rgit scrub`.
pub fn scrub(raw_paths: &[String], dry_run: bool) -> Result<()> {
    crate::workspace::fixup::refuse_if_busy();
    let paths = match normalize_paths(raw_paths) {
        Ok(paths) => paths,
        Err(e) => crate::ui::die_fatal(e.to_string().trim_start_matches("fatal: ")),
    };
    if !dry_run && !backup_refs().is_empty() {
        crate::ui::die_fatal(format!(
            "a previous scrub's backup is still in {}\nhint: check that scrub's result, then `rgit scrub --drop-backup` (or `rgit undo` it) before scrubbing again",
            BACKUP_PREFIX
        ));
    }
    let plan = plan_scrub(&paths, dry_run)?;
    let shown = paths.iter().map(|p| format!("'{}'", p)).collect::<Vec<_>>().join(", ");
    if plan.containing == 0 {
        println!("Nothing to scrub: {} is in none of the {} commits on your branches and tags.", shown, plan.commits);
        return Ok(());
    }
    print_plan(&plan, &shown, dry_run);
    if dry_run {
        println!("Nothing was changed (dry run). Run it without --dry-run to rewrite.");
        return Ok(());
    }
    apply_scrub(&plan)?;
    print_next_steps(&plan);
    Ok(())
}

/// `rgit scrub --drop-backup`: deletes the refs a scrub kept the old history in, and the undo
/// history (which also keeps old commits, and copies of files, for `rgit undo`), so that
/// `reflog expire`, `gc` and `repack -a -d` can then remove every copy of the scrubbed files.
// Used by `rgit scrub --drop-backup`.
pub fn drop_scrub_backup() -> Result<()> {
    let dropped = drop_backup()?;
    if dropped == 0 {
        println!("There is no scrub backup to drop.");
        return Ok(());
    }
    crate::workspace::undo::engine::forget_history()?;
    println!("Dropped the scrub backup ({}) and the undo history (it keeps old commits too).", plural(dropped, "ref", "refs"));
    println!("The old commits can now go: `rgit reflog expire --expire=now --all`, `rgit gc --prune=now` (loose objects)");
    println!("and `rgit repack -a -d` (packed ones).");
    Ok(())
}

/// What the scrub found and what it changes (or would change).
// Used by `scrub`.
fn print_plan(plan: &ScrubPlan, shown: &str, dry_run: bool) {
    let verb = match (dry_run, plan.rewritten) {
        (true, _) => "would be",
        (false, 1) => "was",
        (false, _) => "were",
    };
    println!("{} is in {} of the {} commits on your branches and tags.", shown, plan.containing, plan.commits);
    println!(
        "{} {} rewritten{}.",
        plural(plan.rewritten, "commit", "commits"),
        verb,
        match plan.dropped {
            0 => String::new(),
            n => format!(", and {} left out (they only changed {})", plural(n, "commit", "commits"), if plan.paths.len() == 1 { "that path" } else { "those paths" }),
        }
    );
    let names: Vec<String> = plan.refs.iter().map(|r| short_ref(&r.name)).collect();
    println!("{} {}: {}", plural(names.len(), "ref", "refs"), if dry_run { "would move" } else { "moved" }, names.join(", "));
}

/// The checklist after a real scrub: the secret itself, the remote, and the old copies here.
// Used by `scrub`.
fn print_next_steps(plan: &ScrubPlan) {
    println!("Old branches and tags are saved under {} (`rgit undo` takes the whole scrub back).", BACKUP_PREFIX);
    println!("Files that were tracked stay on disk, now untracked: add them to .gitignore.");
    println!();
    println!("Still to do:");
    println!("  1. If it was a secret, revoke or rotate it: anyone who already cloned or fetched has it.");
    let branches: Vec<String> = plan.refs.iter().filter_map(|r| r.name.strip_prefix("refs/heads/").map(str::to_string)).collect();
    let tags: Vec<String> = plan.refs.iter().filter_map(|r| r.name.strip_prefix("refs/tags/").map(str::to_string)).collect();
    let mut publish: Vec<String> = branches.iter().map(|b| format!("`rgit push --force origin {}`", b)).collect();
    publish.extend(tags.iter().map(|t| format!("`rgit push --force origin {}`", t)));
    println!("  2. Publish the new history: {}.", publish.join(", "));
    println!("  3. Then forget the old copies here: `rgit scrub --drop-backup` (also clears the undo history),");
    println!("     `rgit reflog expire --expire=now --all`, `rgit gc --prune=now`, `rgit repack -a -d`.");
}

fn short_ref(name: &str) -> String {
    name.strip_prefix("refs/heads/")
        .map(str::to_string)
        .or_else(|| name.strip_prefix("refs/tags/").map(|t| format!("tag {}", t)))
        .unwrap_or_else(|| name.to_string())
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}
