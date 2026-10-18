use anyhow::Result;
use crate::helpers::fsck::run_fsck_checks;
use crate::objects::FsckSeverity;

/// `rgit fsck [--connectivity-only] [--no-dangling] [--unreachable] [--no-reflogs]`
///
/// Verifies the object store: every loose object must decompress, declare the right size,
/// hash to the name it is filed under and be well-formed for its type (commits, trees, tags);
/// every object reachable from a ref, HEAD, the index, the stash, an operation in progress or
/// a reflog entry must exist; refs, HEAD and the index must be valid. Problems go to stderr as
/// `error:` / `warning:` lines and the exit status is 1 if there were any errors, so it can be
/// used in scripts. Objects nothing reaches are listed as `dangling <type> <hash>` (the tips of
/// abandoned work; `rgit show <hash>` to look, `rgit branch`/`reset` to rescue). With
/// `--unreachable` every unreachable object is listed instead. `--connectivity-only` skips the
/// per-object hash check, and `--no-reflogs` stops reflog entries from counting as roots.
pub fn fsck(connectivity_only: bool, no_dangling: bool, unreachable: bool, no_reflogs: bool) -> Result<()> {
    let report = run_fsck_checks(connectivity_only, !no_reflogs)?;

    for issue in &report.issues {
        match issue.severity {
            FsckSeverity::Error => eprintln!("error: {}", issue.message),
            FsckSeverity::Warning => eprintln!("warning: {}", issue.message),
        }
    }

    if unreachable {
        for (kind, hash) in &report.unreachable {
            println!("unreachable {} {}", kind, hash);
        }
    } else if !no_dangling {
        for (kind, hash) in &report.dangling {
            println!("dangling {} {}", kind, hash);
        }
    }

    println!(
        "Checked {} object{}: {} error{}, {} warning{}, {} dangling.",
        report.objects_checked,
        if report.objects_checked == 1 { "" } else { "s" },
        report.error_count(),
        if report.error_count() == 1 { "" } else { "s" },
        report.warning_count(),
        if report.warning_count() == 1 { "" } else { "s" },
        report.dangling.len()
    );

    if report.error_count() > 0 {
        std::process::exit(1);
    }
    Ok(())
}