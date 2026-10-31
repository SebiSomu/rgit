use anyhow::Result;

use crate::helpers::pack_refs::{pack_refs_matching, parse_pack_refs_args, PACK_REFS_USAGE};
use crate::repo;

/// `rgit pack-refs [--all] [--no-prune] [--include <pattern>] [--exclude <pattern>]`
///
/// Moves loose refs into `.git/packed-refs`, git's single-file ref store, so repositories with
/// many refs don't need a file per ref. By default only tags are packed; `--all` packs every
/// branch, remote-tracking branch and other ref too, and `--include`/`--exclude` select refs by
/// glob (excludes win). Symbolic refs, per-worktree refs (`refs/bisect/...`) and broken refs
/// stay loose. Annotated tags get a `^<peeled>` line, and the loose files are deleted afterwards
/// unless `--no-prune` (their reflogs are kept). Every rgit command already reads packed refs,
/// so nothing changes for them. Exit status, like git: 0 success, 128 fatal, 129 usage.
pub fn pack_refs(args: Vec<String>) -> Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{}", PACK_REFS_USAGE);
        std::process::exit(129);
    }
    let opts = match parse_pack_refs_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            eprint!("{}", e);
            std::process::exit(129);
        }
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        std::process::exit(128);
    }
    if let Err(e) = pack_refs_matching(&opts) {
        let message = format!("{:#}", e);
        eprintln!("{}", if message.starts_with("fatal: ") { message } else { format!("fatal: {}", message) });
        std::process::exit(128);
    }
    Ok(())
}
