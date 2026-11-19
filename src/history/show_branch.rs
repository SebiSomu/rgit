pub mod engine;
pub mod types;

use anyhow::Result;

use crate::history::show_branch::engine::{collect_show_branch_names, parse_show_branch_args, show_branch_output, SHOW_BRANCH_USAGE};
use crate::core::repo;

/// `rgit show-branch [-a | --all] [-r | --remotes] [--current] [--topo-order | --date-order]
/// [--more=<n> | --list | --independent | --merge-base] [--no-name | --sha1-name]
/// [--topics] [--sparse] [(<rev> | <glob>)...]`
///
/// Shows how branches relate. Each named tip (all local branches by default; `-r` adds
/// remote-tracking branches, `-a` both; a glob such as `topic/*` matches refs; `--current`
/// adds the current branch) gets a header line and a column. Under a line of dashes come
/// the commits on those branches down to their common ancestor, newest first, each with a
/// `+` in the column of every tip that contains it (`*` in the current branch's column, `-`
/// for merges) and a name relative to a tip such as `[topic~2]`. `--more=<n>` goes `n`
/// commits past the common ancestor, `--list` prints only the headers, `--sha1-name` shows
/// abbreviated hashes instead of names and `--no-name` nothing, `--topics` shows only
/// commits not on the first tip, `--sparse` keeps merges that only one tip contains,
/// `--date-order` orders by date rather than topology, `--merge-base` prints the merge
/// bases of the tips (exit 1 if none), and `--independent` the tips no other tip contains.
/// At most 26 tips. `showbranch.default` supplies arguments when none are given.
/// `--reflog` is not supported. Exit status, like git: 0, 128 fatal, 129 usage.
pub fn show_branch(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        print!("{}", SHOW_BRANCH_USAGE);
        crate::ui::exit(129);
    }
    if !repo::exists() {
        fatal("fatal: not a git repository (or any of the parent directories): .git");
    }
    let opts = match parse_show_branch_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            eprint!("{}", message);
            crate::ui::exit(if message.starts_with("fatal:") { 128 } else { 129 });
        }
    };
    let (names, head) = collect_show_branch_names(&opts).unwrap_or_else(|e| fatal(&e.to_string()));
    if names.is_empty() {
        eprintln!("No revs to be shown.");
        return Ok(());
    }
    match show_branch_output(&opts, &names, head.as_deref()) {
        Ok((output, status)) => {
            print!("{}", output);
            if status != 0 {
                crate::ui::exit(status);
            }
            Ok(())
        }
        Err(e) => fatal(&e.to_string()),
    }
}

fn fatal(message: &str) -> ! {
    eprintln!("{}", message);
    crate::ui::exit(128);
}
