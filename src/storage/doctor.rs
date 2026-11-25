pub mod cli;
pub mod engine;
pub mod types;

use crate::storage::doctor::engine::{apply, check_config, check_hooks, check_identity, check_large_files, check_locks, check_objects, check_operations, check_refs, check_worktrees};
use crate::storage::doctor::types::{Finding, Severity};

/// `rgit doctor [--fix] [--quick] [--large <size>]`: checks the repository for problems
/// (objects, refs, identity, config, hooks, stale lock files, broken worktrees, huge untracked
/// files, operations left in progress) and prints each with the command that fixes it. With
/// `--fix` it also applies the safe fixes itself (stale locks, worktree records, hook
/// permissions). Exits 1 when an error is left.
pub fn doctor(fix: bool, quick: bool, large: u64) -> anyhow::Result<()> {
    let mut findings: Vec<Finding> = Vec::new();
    let mut areas = 0;
    let mut run = |found: Vec<Finding>| {
        areas += 1;
        findings.extend(found);
    };
    if !quick {
        run(check_objects()?);
    }
    run(check_refs());
    run(check_identity()?);
    run(check_config()?);
    run(check_hooks());
    run(check_locks());
    run(check_worktrees()?);
    run(check_large_files(large)?);
    run(check_operations());

    let mut fixed: Vec<String> = Vec::new();
    if fix {
        let mut pruned = false;
        let mut kept = Vec::new();
        for finding in findings {
            match &finding.auto {
                Some(crate::storage::doctor::types::AutoFix::PruneWorktrees) if pruned => {}
                Some(auto) => match apply(auto) {
                    Ok(done) => {
                        pruned |= *auto == crate::storage::doctor::types::AutoFix::PruneWorktrees;
                        fixed.push(done);
                    }
                    Err(e) => {
                        eprintln!("error: couldn't fix it: {}", e);
                        kept.push(finding);
                    }
                },
                None => kept.push(finding),
            }
        }
        findings = kept;
    }

    for finding in &findings {
        let label = match finding.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        println!("{:<9}{}", label, finding.message);
        if let Some(how) = &finding.fix {
            println!("{:<9}fix: {}", "", how);
        }
    }
    for done in &fixed {
        println!("{:<9}{}", "fixed", done);
    }
    let errors = findings.iter().filter(|f| f.severity == Severity::Error).count();
    let warnings = findings.len() - errors;
    let checked = format!("Checked {} areas{}", areas, if quick { " (objects skipped: --quick)" } else { "" });
    let count = |n: usize, what: &str| format!("{} {}{}", n, what, if n == 1 { "" } else { "s" });
    let mut parts: Vec<String> = Vec::new();
    if !fixed.is_empty() {
        parts.push(format!("{} fixed", fixed.len()));
    }
    if errors > 0 {
        parts.push(count(errors, "error"));
    }
    if warnings > 0 {
        parts.push(count(warnings, "warning"));
    }
    if parts.is_empty() {
        println!("{}: no problems found.", checked);
    } else {
        println!("{}: {}.", checked, parts.join(", "));
    }
    let auto_left = findings.iter().any(|f| f.auto.is_some());
    if auto_left && !fix {
        println!("`rgit doctor --fix` applies the fixes marked `rgit doctor --fix`.");
    }
    if errors > 0 {
        crate::ui::exit(1);
    }
    Ok(())
}
