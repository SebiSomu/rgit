use anyhow::Result;

use crate::helpers::shortlog::collect_shortlog;
use crate::objects::ShortlogOptions;

/// `rgit shortlog [-s|--summary] [-n|--numbered] [-e|--email] [--all] [<rev>]`
///
/// Summarises `git log` output grouped by author.
/// Groups all commits reachable from `<rev>` (or HEAD by default, or all refs with `--all`)
/// by author name (or name+email with `-e`).
///
/// If `-s` is specified, each author is printed on a single line prefixed by the commit count.
/// Otherwise, each author header `Author Name (count):` is followed by each commit's subject line,
/// indented by 6 spaces.
pub fn shortlog(opts: ShortlogOptions) -> Result<()> {
    let entries = collect_shortlog(&opts)?;

    if opts.summary {
        for entry in entries {
            println!("{:>6}\t{}", entry.subjects.len(), entry.display);
        }
    } else {
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                println!();
            }
            println!("{} ({}):", entry.display, entry.subjects.len());
            for subject in &entry.subjects {
                println!("      {}", subject);
            }
        }
    }

    Ok(())
}