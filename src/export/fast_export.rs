pub mod engine;
pub mod types;

use std::io::{self, Write};
use anyhow::Result;

use crate::export::fast_export::engine::{parse_fast_export_args, run_fast_export, FAST_EXPORT_USAGE};
use crate::core::repo;

/// `rgit fast-export [--all | <rev>...] [--signed-tags=<mode>] [--no-data] [--full-tree]
/// [--export-marks=<file>] [--import-marks=<file>] [--use-done-feature] [--mark-tags]
/// [--reference-excluded-parents] [--show-original-ids] [--tag-of-filtered-object=<mode>]`
///
/// Writes the selected history to standard output as a `fast-import` stream: a plain-text
/// description of blobs, commits (with their file changes against the first parent), refs
/// and annotated tags that `git fast-import` / `rgit fast-import` can replay, and that is
/// easy to filter with a script in between. Revisions are given as for `rev-list` (`--all`,
/// `main`, `A..B`, `^old new`). Marks (`:1`, `:2`, ...) name the objects inside the stream;
/// `--export-marks` saves the mark-to-object table and `--import-marks` loads one, so an
/// export can be continued incrementally. A commit whose parent is not part of the export
/// is written with its whole tree and without that parent, unless
/// `--reference-excluded-parents` names it by hash. `--no-data` leaves the blob contents
/// out and refers to blobs by hash. Signed tags stop the export unless `--signed-tags` says
/// `verbatim`, `warn`, `warn-strip` or `strip`. Not supported: rename/copy detection
/// (`-M`, `-C`), `--anonymize`, `--refspec`, `--reencode` and path limiting. Exit status: 0,
/// 128 on errors, 129 for a usage error.
pub fn fast_export(args: Vec<String>) -> Result<()> {
    let opts = match parse_fast_export_args(&args) {
        Ok(opts) => opts,
        Err((message, code)) => {
            if message.starts_with("usage:") {
                print!("{}", message);
            } else {
                eprint!("{}", message);
            }
            crate::ui::exit(code);
        }
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    if opts.revs.is_empty() {
        eprint!("{}", FAST_EXPORT_USAGE);
        crate::ui::exit(129);
    }
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    if let Err(e) = run_fast_export(&opts, &mut out) {
        out.flush()?;
        eprintln!("{}", e);
        crate::ui::exit(128);
    }
    out.flush()?;
    Ok(())
}
