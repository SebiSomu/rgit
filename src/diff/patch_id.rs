pub mod engine;
pub mod types;

use std::io::{self, Read, Write};
use anyhow::Result;

use crate::diff::patch_id::engine::{compute_patch_ids, parse_patch_id_args, PATCH_ID_USAGE};

/// `rgit patch-id [--stable | --unstable | --verbatim]`
///
/// Reads patches from standard input (`diff`, `log -p`, `show` or `format-patch` output) and
/// prints `<patch-id> <commit-id>` for each one. A patch ID is a SHA-1 over the diff with
/// line numbers and whitespace left out, so the same change gets the same ID wherever it is
/// applied. `--unstable` (the default) hashes the whole patch in one go; `--stable` hashes
/// every file separately and sums the results, so the order of files doesn't matter (this is
/// the ID `cherry` uses); `--verbatim` is `--stable` without stripping whitespace. The
/// defaults come from `patchid.stable` / `patchid.verbatim`. The commit id is all zeros when
/// the input doesn't name one. Works outside a repository. Exits 129 on usage errors.
pub fn patch_id(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h" || a == "--help") {
        print!("{}", PATCH_ID_USAGE);
        crate::ui::exit(129);
    }
    let opts = match parse_patch_id_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            eprint!("{}", e);
            crate::ui::exit(129);
        }
    };

    let mut input = Vec::new();
    io::stdin().lock().read_to_end(&mut input)?;

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for (patch, commit) in compute_patch_ids(&input, &opts) {
        writeln!(out, "{} {}", patch, commit)?;
    }
    Ok(out.flush()?)
}
