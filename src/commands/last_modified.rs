use std::io::{self, Write};
use anyhow::Result;

use crate::helpers::last_modified::{last_modified_paths, parse_last_modified_args, LAST_MODIFIED_USAGE};
use crate::core::repo;

/// `rgit last-modified [-r | --recursive] [-t | --show-trees] [-z] [<revision-range>] [[--] <path>...]`
///
/// Shows, for each path in the tree of `<revision-range>`'s newest commit (default
/// `HEAD`), the commit that last modified it, one `<commit>\t<path>` line per path: in
/// effect a `log -1 -- <path>` for every entry at once. Only top-level entries are listed
/// unless `-r` recurses into directories; `-t` also lists the directories themselves.
/// Path arguments limit the listing (a directory argument lists that directory's entries).
/// With a range such as `v1..HEAD`, paths the range never touched are shown against the
/// boundary commit with a `^` before its hash. `-z` ends entries with NUL instead of a
/// newline. This is the command git added in 2.52; rgit's output is ordered by path.
pub fn last_modified(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        print!("{}", LAST_MODIFIED_USAGE);
        crate::ui::exit(129);
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    let opts = match parse_last_modified_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            eprint!("{}{}", message, if message.ends_with('\n') { "" } else { "\n" });
            crate::ui::exit(if message.starts_with("error: unknown option") { 129 } else { 128 });
        }
    };
    let entries = match last_modified_paths(&opts) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("{}", e);
            crate::ui::exit(128);
        }
    };
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for (commit, path, boundary) in entries {
        write!(out, "{}{}\t{}{}", if boundary { "^" } else { "" }, commit, path, if opts.nul { '\0' } else { '\n' })?;
    }
    Ok(out.flush()?)
}
