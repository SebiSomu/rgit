use anyhow::Result;

use crate::helpers::replay::{parse_replay_args, replay_commits, REPLAY_USAGE};

/// `rgit replay ([--contained] --onto <newbase> | --advance <branch>) <revision-range>...`
///
/// Replays commits onto a new base without touching the working tree, the index or any
/// ref (git 2.44's command): the commits of the ranges are re-created in the object store
/// on top of `<newbase>`, like a rebase, and the result is printed as `update <ref> <new>
/// <old>` lines to pipe into `rgit update-ref --stdin`. `--onto` moves the branches named
/// as range tips (`--onto main main..topic` rebases `topic`; `--contained` also moves
/// every other branch pointing into the range). `--advance <branch>` instead puts the
/// commits on top of `<branch>` and moves that branch, like a cherry-pick. Merge commits
/// can't be replayed. Exit status, like git: 0 success, 1 a commit conflicts (nothing is
/// printed), 128 fatal, 129 usage.
pub fn replay(args: Vec<String>) -> Result<()> {
    if args.iter().any(|a| a == "-h") {
        print!("{}", REPLAY_USAGE);
        crate::ui::exit(129);
    }
    if !crate::repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    let opts = match parse_replay_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            eprint!("{}", message);
            crate::ui::exit(if message.starts_with("fatal:") { 128 } else { 129 });
        }
    };
    match replay_commits(&opts) {
        Ok(Some(updates)) => {
            for line in updates {
                println!("{}", line);
            }
            Ok(())
        }
        Ok(None) => crate::ui::exit(1),
        Err(e) => {
            eprintln!("{}", e);
            crate::ui::exit(128);
        }
    }
}
