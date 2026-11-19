pub mod engine;

use std::fs;
use std::io::{self, Read, Write};
use anyhow::Result;

use crate::mail::interpret_trailers::engine::{parse_interpret_trailers_args, process_trailers, read_trailer_config, write_trailers_in_place, INTERPRET_TRAILERS_USAGE};

/// `rgit interpret-trailers [--in-place] [--trim-empty] [--where <placement>]
/// [--if-exists <action>] [--if-missing <action>] [(--trailer <token>[(=|:)<value>])...]
/// [--only-trailers] [--only-input] [--unfold] [--parse] [--no-divider] [<file>...]`
///
/// Adds or parses trailers, the `Token: value` lines (such as `Signed-off-by:`) in the last
/// paragraph of a commit message. Reads the message from the files (or standard input) and
/// prints it with the `--trailer` arguments applied; `--in-place` rewrites the files
/// instead. A new trailer goes at the end of the trailer block unless `--where` says
/// `after`, `before` or `start`; when its token already exists `--if-exists` decides
/// (`addIfDifferentNeighbor` by default, `addIfDifferent`, `add`, `replace`, `doNothing`),
/// and when it doesn't `--if-missing` does (`add` or `doNothing`). These three apply to the
/// `--trailer` options after them and default to `trailer.where`, `trailer.ifexists` and
/// `trailer.ifmissing`. `trailer.<token>.key` gives a token its canonical spelling (and lets
/// it be abbreviated), `trailer.<token>.command` / `.cmd` compute its value with a shell
/// command, and `trailer.separators` lists the accepted separators. `--only-trailers`
/// prints just the trailers, `--only-input` ignores `--trailer` and configured commands,
/// `--unfold` joins continuation lines, `--parse` is all three, `--trim-empty` drops
/// trailers without a value, and `--no-divider` stops treating `---` as the end of the
/// message. Works outside a repository. Exits 129 on usage errors and 128 when a file
/// can't be read or replaced.
pub fn interpret_trailers(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        print!("{}", INTERPRET_TRAILERS_USAGE);
        crate::ui::exit(129);
    }
    let opts = match parse_interpret_trailers_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            eprint!("{}", e);
            crate::ui::exit(129);
        }
    };
    let config = read_trailer_config();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.files.is_empty() {
        if opts.in_place {
            fatal("fatal: no input file given for in-place editing");
        }
        let mut message = Vec::new();
        if io::stdin().lock().read_to_end(&mut message).is_err() {
            fatal("fatal: could not read from stdin");
        }
        out.write_all(&process_trailers(&message, &opts, &config))?;
        return Ok(out.flush()?);
    }

    for file in &opts.files {
        let message = fs::read(file).unwrap_or_else(|e| {
            let reason = match e.kind() {
                io::ErrorKind::NotFound => "No such file or directory".to_string(),
                io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
                _ if fs::metadata(file).is_ok_and(|m| m.is_dir()) => "Is a directory".to_string(),
                _ => e.to_string(),
            };
            fatal(&format!("fatal: could not read input file '{}': {}", file, reason))
        });
        let result = process_trailers(&message, &opts, &config);
        if opts.in_place {
            if let Err(e) = write_trailers_in_place(file, &result) {
                fatal(&e.to_string());
            }
        } else {
            out.write_all(&result)?;
        }
    }
    Ok(out.flush()?)
}

fn fatal(message: &str) -> ! {
    eprintln!("{}", message);
    crate::ui::exit(128);
}
