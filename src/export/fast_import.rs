pub mod engine;
pub mod types;

use std::io::{self, Read};
use anyhow::Result;

use crate::export::fast_import::engine::{finish_fast_import, import_marks, run_fast_import, set_date_format};
use crate::export::fast_import::types::FastImportState;
use crate::core::repo;

/// `rgit fast-import [--force] [--quiet] [--stats] [--done] [--date-format=<format>]
/// [--export-marks=<file>] [--import-marks=<file>] [--import-marks-if-exists=<file>]`
///
/// Reads a `fast-import` stream from standard input (as written by `fast-export`, by
/// `git fast-export`, or by a conversion script) and creates the blobs, trees, commits and
/// tags it describes, then points the refs at them. Supported commands: `blob`, `commit`
/// (file changes `M` with a mark, an object name or `inline` data; `D`; `C`; `R`;
/// `deleteall`; `N` for notes), `tag`, `reset`, `alias`, `progress`, `checkpoint`,
/// `get-mark`, `feature`, `option` and `done`. Dates are `<seconds> <timezone>`
/// (`--date-format=now` also exists). A branch that already exists is only moved forward:
/// if the imported tip does not contain the current one, the branch is left alone with a
/// warning, unless `--force`. `--export-marks` / `--import-marks` save and load the
/// mark table for incremental imports. Unlike git, objects are written as loose objects
/// rather than into a packfile (run `repack` afterwards), statistics are only printed with
/// `--stats`, and `cat-blob` / `ls` are not available. `HEAD`, the index and the working
/// tree are not touched. Exit status: 0, 1 when a ref was not updated, 128 on errors,
/// 129 for a usage error.
pub fn fast_import(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit fast-import [--date-format=<f>] [--force] [--quiet] [--stats] [--done] [--export-marks=<marks.file>] [--import-marks=<marks.file>]\n";
    let die = |message: &str, code: i32| -> ! {
        eprint!("{}", message);
        crate::ui::exit(code);
    };
    if !repo::exists() {
        die("fatal: not a git repository (or any of the parent directories): .git\n", 128);
    }
    let mut state = FastImportState { date_format: "raw".to_string(), ..Default::default() };
    let mut stats = false;
    for arg in &args {
        let (name, value) = arg.split_once('=').unwrap_or((arg.as_str(), ""));
        let applied = match name {
            "--force" => {
                state.force = true;
                Ok(())
            }
            "--quiet" => {
                stats = false;
                Ok(())
            }
            "--stats" => {
                stats = true;
                Ok(())
            }
            "--done" => {
                state.require_done = true;
                Ok(())
            }
            "--date-format" => set_date_format(&mut state, value),
            "--export-marks" => {
                state.export_marks = Some(value.to_string());
                Ok(())
            }
            "--import-marks" => import_marks(&mut state, value, false),
            "--import-marks-if-exists" => import_marks(&mut state, value, true),
            "--relative-marks" | "--no-relative-marks" | "--max-pack-size" | "--big-file-threshold" | "--depth" | "--active-branches" | "--export-pack-edges" | "--allow-unsafe-features" | "--signed-commits" | "--signed-tags" => Ok(()),
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            "--cat-blob-fd" | "--rewrite-submodules-from" | "--rewrite-submodules-to" => die(&format!("fatal: {} is not supported by rgit fast-import\n", name), 128),
            _ => die(&format!("fatal: unknown option {}\n{}", arg, USAGE), 129),
        };
        if let Err(e) = applied {
            die(&format!("{}\n", e), 128);
        }
    }

    let mut data = Vec::new();
    io::stdin().lock().read_to_end(&mut data)?;
    if let Err(e) = run_fast_import(&data, &mut state) {
        die(&format!("{}\n", e), 128);
    }
    let skipped = finish_fast_import(&state)?;
    if stats {
        eprintln!("rgit fast-import statistics:");
        eprintln!("      blobs  : {:>10}", state.counts[0]);
        eprintln!("      trees  : {:>10}", state.counts[1]);
        eprintln!("      commits: {:>10}", state.counts[2]);
        eprintln!("      tags   : {:>10}", state.counts[3]);
        eprintln!("      marks  : {:>10}", state.marks.len());
        eprintln!("      refs   : {:>10}", state.refs.values().filter(|tip| tip.is_some()).count());
    }
    if skipped {
        crate::ui::exit(1);
    }
    Ok(())
}
