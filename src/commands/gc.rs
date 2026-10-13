use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::helpers::gc::{
    expire_all_reflogs, expiry_cutoff, format_byte_size, plan_gc, remove_loose_object,
};

/// `rgit gc [--prune <days|now|never>] [--reflog-expire <days|now|never>] [-n]`
///
/// Housekeeping for the object store: expires reflog entries older than `--reflog-expire`
/// (default 90 days), then deletes unreachable loose objects older than `--prune` (default 14
/// days). An object counts as reachable if a branch, tag, remote-tracking branch, HEAD, the
/// index, the stash, an operation in progress (merge, rebase, bisect, ...) or a live reflog
/// entry leads to it, so work that was only reset, amended or rebased away stays recoverable
/// until its reflog entries expire. Objects younger than the prune window, and whatever they
/// refer to, are always kept. If a reachable object is missing or unreadable nothing is
/// deleted at all. `-n` shows what would be done and changes nothing.
///
/// There are no packfiles to repack, so this reclaims space but never compacts it.
pub fn gc(prune: String, reflog_expire: String, dry_run: bool) -> Result<()> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let prune_cutoff = expiry_cutoff(&prune, now)?;
    let reflog_cutoff = expiry_cutoff(&reflog_expire, now)?;
    let mut report = plan_gc(prune_cutoff, reflog_cutoff)?;
    report.reflog_entries_expired = expire_all_reflogs(reflog_cutoff, dry_run)?;

    if !dry_run {
        for object in &report.pruned {
            remove_loose_object(object)?;
        }
    }

    println!(
        "Checked {} loose object{}: {} reachable, {} unreachable.",
        report.total_objects,
        if report.total_objects == 1 { "" } else { "s" },
        report.reachable_objects,
        report.unreachable_objects
    );

    if report.reflog_entries_expired > 0 {
        println!(
            "{} {} reflog entr{}.",
            if dry_run { "Would expire" } else { "Expired" },
            report.reflog_entries_expired,
            if report.reflog_entries_expired == 1 { "y" } else { "ies" }
        );
    }

    if report.unreachable_objects > 0 {
        println!(
            "{} {} unreachable object{} ({}); kept {} that {} still needed or too new.",
            if dry_run { "Would prune" } else { "Pruned" },
            report.pruned.len(),
            if report.pruned.len() == 1 { "" } else { "s" },
            format_byte_size(report.pruned_bytes()),
            report.kept_unreachable,
            if report.kept_unreachable == 1 { "is" } else { "are" }
        );
    }

    if dry_run {
        const LISTED: usize = 50;
        for object in report.pruned.iter().take(LISTED) {
            println!("  would prune {} ({})", object.hash, format_byte_size(object.size));
        }
        if report.pruned.len() > LISTED {
            println!("  ... and {} more", report.pruned.len() - LISTED);
        }
        println!("(dry run: nothing was changed)");
    }

    Ok(())
}