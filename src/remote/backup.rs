pub mod cli;
pub mod engine;
pub mod types;

use crate::remote::backup::engine::{backup_dir, list_backups, repo_name, shown, size, stamp_now};

/// Backups of one repository kept when neither `--keep` nor `rgit.backupKeep` says.
const DEFAULT_KEEP: usize = 10;

/// `rgit backup [--dir <dir>] [--keep <n>] [-f] | --list`: writes a bundle of every ref
/// (`bundle create --all`: branches, tags, remote-tracking refs, the stash, `rgit wip` saves)
/// to `<dir>/<repo>-<UTC time>.bundle`, the directory being `--dir`, `rgit.backupDir` or
/// `~/.rgit-backups`. Skipped when the newest backup already has exactly the same refs
/// (`-f` makes one anyway). Then only the newest `--keep` backups of this repository are kept.
/// `rgit clone <file> <dir>` restores one. Uncommitted changes aren't refs, so they are not
/// included (it says so).
pub fn backup(dir: Option<String>, keep: Option<usize>, force: bool, list: bool) -> anyhow::Result<()> {
    let Some(dir) = backup_dir(dir) else {
        crate::ui::die_fatal("no backup directory: HOME is not set; pass --dir or set rgit.backupDir");
    };
    let name = repo_name();
    let existing = list_backups(&dir, &name);

    if list {
        if existing.is_empty() {
            println!("No backups of {} in {}.", name, shown(&dir));
            return Ok(());
        }
        println!("Backups of {} in {} (newest first):", name, shown(&dir));
        for b in existing.iter().rev() {
            let refs = crate::remote::bundle::engine::read_bundle(&b.path.to_string_lossy()).map(|(h, _)| h.refs.len()).unwrap_or(0);
            let bytes = std::fs::metadata(&b.path).map(|m| m.len()).unwrap_or(0);
            println!("  {}  {:>8}  {} ref{}", b.path.file_name().unwrap_or_default().to_string_lossy(), size(bytes), refs, if refs == 1 { "" } else { "s" });
        }
        println!("Restore one with `rgit clone <file> <dir>`.");
        return Ok(());
    }

    if crate::core::refs::resolve_head_commit()?.is_none() && crate::core::refs::list_all_refs().is_empty() {
        crate::ui::die_fatal("there is nothing to back up yet: no commits");
    }
    let (refs, contents, objects) = crate::remote::bundle::build_bundle(2, &["--all".to_string()])?;
    let unchanged = existing.last().and_then(|b| crate::remote::bundle::engine::read_bundle(&b.path.to_string_lossy()).ok()).is_some_and(|(h, _)| {
        let mut theirs = h.refs.clone();
        let mut ours = refs.clone();
        theirs.sort();
        ours.sort();
        theirs == ours
    });
    if unchanged && !force {
        let last = existing.last().expect("checked");
        println!("Nothing new since the last backup ({}): same {} ref{}. `-f` makes one anyway.", shown(&last.path), refs.len(), if refs.len() == 1 { "" } else { "s" });
        return Ok(());
    }

    std::fs::create_dir_all(&dir).map_err(|e| anyhow::anyhow!("fatal: could not create {}: {}", shown(&dir), e))?;
    let stamp = stamp_now();
    let sequence = existing.iter().filter(|b| b.stamp == stamp).map(|b| b.sequence).max().map(|n| n + 1);
    let file = dir.join(match sequence {
        Some(n) => format!("{}-{}-{}.bundle", name, stamp, n),
        None => format!("{}-{}.bundle", name, stamp),
    });
    let lock = file.with_extension("bundle.lock");
    std::fs::write(&lock, &contents).map_err(|e| anyhow::anyhow!("fatal: could not write {}: {}", shown(&file), e))?;
    std::fs::rename(&lock, &file)?;
    println!(
        "Backed up {} ref{} ({} object{}) to {} ({}).",
        refs.len(),
        if refs.len() == 1 { "" } else { "s" },
        objects,
        if objects == 1 { "" } else { "s" },
        shown(&file),
        size(contents.len() as u64)
    );

    let keep = keep.or_else(|| crate::core::config::get_config_value("rgit.backupkeep").ok().flatten().and_then(|v| v.parse().ok())).unwrap_or(DEFAULT_KEEP);
    let all = list_backups(&dir, &name);
    if keep > 0 && all.len() > keep {
        let old = &all[..all.len() - keep];
        let mut removed = Vec::new();
        for b in old {
            if std::fs::remove_file(&b.path).is_ok() {
                removed.push(b.path.file_name().unwrap_or_default().to_string_lossy().to_string());
            }
        }
        if !removed.is_empty() {
            println!("Removed {} older backup{} (keeping the newest {}): {}", removed.len(), if removed.len() == 1 { "" } else { "s" }, keep, removed.join(", "));
        }
    }
    let dirty = !crate::core::commit::tracked_worktree_changes()?.is_empty() || crate::workspace::fixup::engine::has_staged_changes(&crate::core::refs::resolve_head_commit()?.unwrap_or_default()).unwrap_or(false);
    if dirty {
        println!("Not in it: your uncommitted changes (`rgit wip` saves them as a ref; back up again after).");
    }
    println!("Restore with `rgit clone {} <dir>`.", shown(&file));
    Ok(())
}
