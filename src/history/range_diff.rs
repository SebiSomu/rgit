pub mod engine;
pub mod types;

use anyhow::Result;

use crate::history::range_diff::engine::{format_range_diff, match_range_patches, parse_range_diff_args, read_range_patches, RANGE_DIFF_USAGE};
use crate::core::repo;

/// `rgit range-diff [--creation-factor=<n>] [--left-only | --right-only] [-s | --no-patch]
/// (<range1> <range2> | <rev1>...<rev2> | <base> <rev1> <rev2>)`
///
/// Compares two versions of a patch series, for example a branch before and after a
/// rebase. Each commit of the two ranges (merges excluded) is turned into its patch, and
/// the patches are paired up: identical ones first, then the closest ones, as long as the
/// "diff of diffs" is smaller than `--creation-factor` percent (default 60) of treating
/// them as unrelated. The output lists the pairs in the order of the second range as
/// `<n>:  <hash> <status> <m>:  <hash> <subject>` with status `=` (unchanged), `!`
/// (changed), `<` (only in the first range) or `>` (only in the second). A changed pair is
/// followed by a comparison of the two patches, where the first column shows what changed
/// between the versions and the second is the patch's own `+`/`-`; `-s` leaves it out.
/// `A...B` compares `B..A` with `A..B`, and `<base> <rev1> <rev2>` compares `<base>..<rev1>`
/// with `<base>..<rev2>`. Output is never coloured. Exit status, like git: 0 success, 128
/// fatal, 129 usage.
pub fn range_diff(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        print!("{}", RANGE_DIFF_USAGE);
        crate::ui::exit(129);
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    let opts = match parse_range_diff_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            if message.starts_with("fatal:") {
                eprintln!("{}", message);
                crate::ui::exit(128);
            }
            eprint!("{}", message.strip_prefix("usage-error: ").unwrap_or(&message));
            crate::ui::exit(129);
        }
    };

    let load = |range: &str| {
        read_range_patches(range).unwrap_or_else(|e| {
            eprintln!("{}", e);
            eprintln!("error: could not parse log for '{}'", range);
            crate::ui::exit(255);
        })
    };
    let mut a = load(&opts.range1);
    let mut b = load(&opts.range2);
    match_range_patches(&mut a, &mut b, opts.creation_factor);
    print!("{}", format_range_diff(&mut a, &b, &opts));
    Ok(())
}
