use anyhow::Result;

use crate::helpers::rerere::{conflicted_paths, read_merge_rr, record_and_replay, rerere_clear, rerere_diff, rerere_enabled, rerere_forget, rerere_gc, RERERE_USAGE};

/// `rgit rerere [clear | forget <path>... | diff | status | remaining | gc]`
///
/// Reuses recorded resolutions of conflicted merges ("reuse recorded resolution"). Run
/// with no arguments after a merge, rebase or cherry-pick stops on conflicts: each
/// conflict is fingerprinted (independent of branch names and of which side is which) and
/// its conflicted form saved under `.git/rr-cache/<id>/preimage`. Run it again once the
/// files are resolved and the resolutions are saved as `postimage`. The next time the same
/// conflict appears, `rgit rerere` applies the saved resolution to the file. The cache uses
/// git's format, so recordings are shared with git. Unlike git, rgit's `merge`, `rebase`
/// and `commit` don't call rerere themselves, and conflicts are found by their markers in
/// tracked files, since rgit's index has no conflict stages. `status` lists the paths
/// being tracked, `remaining` those still in conflict, `diff` shows the resolution so far
/// against the recorded conflict, `forget <path>...` drops a recorded resolution, `clear`
/// abandons the current recordings, and `gc` prunes old ones (`gc.rerereResolved`, 60
/// days; `gc.rerereUnresolved`, 15 days). Nothing happens unless `rerere.enabled` is true
/// or `.git/rr-cache` exists. Exit status: 0 success, 1 failure, 129 usage.
pub fn rerere(args: Vec<String>) -> Result<()> {
    let usage = || -> ! {
        eprint!("{}", RERERE_USAGE);
        crate::ui::exit(129);
    };
    let args: Vec<&String> = args.iter().filter(|a| !matches!(a.as_str(), "--rerere-autoupdate" | "--no-rerere-autoupdate")).collect();
    if args.iter().any(|a| a.as_str() == "-h") {
        usage();
    }
    if !crate::core::repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    let result: Result<()> = (|| {
        match args.split_first() {
            None => {
                if rerere_enabled() {
                    for message in record_and_replay()? {
                        eprintln!("{}", message);
                    }
                }
            }
            Some((sub, rest)) => match (sub.as_str(), rest.is_empty()) {
                ("forget", false) => {
                    let paths: Vec<String> = rest.iter().filter(|p| p.as_str() != "--").map(|p| (*p).clone()).collect();
                    for message in rerere_forget(&paths)? {
                        eprintln!("{}", message);
                    }
                }
                ("forget", true) => eprintln!("warning: 'git rerere forget' without paths is deprecated"),
                ("clear", true) => rerere_clear(),
                ("gc", true) => rerere_gc(),
                ("status", true) => read_merge_rr().iter().for_each(|(_, path)| println!("{}", path)),
                ("remaining", true) => conflicted_paths()?.iter().for_each(|path| println!("{}", path)),
                ("diff", true) => print!("{}", rerere_diff()),
                _ => usage(),
            },
        }
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("{}", e);
        crate::ui::exit(1);
    }
    Ok(())
}
