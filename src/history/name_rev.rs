pub mod engine;

use std::io::{self, BufRead, Write};
use anyhow::Result;

use crate::history::name_rev::engine::{annotate_line, build_rev_names, commit_parents_and_date, object_name, parse_name_rev_args, peel_ref_tip, CUTOFF_DATE_SLOP, NAME_REV_USAGE};
use crate::history::rev_list::engine::abbreviate_hash;
use crate::core::rev_parse::resolve_rev;
use crate::core::repo;

/// `rgit name-rev [--name-only] [--tags] [--refs=<pattern>]... [--exclude=<pattern>]...
/// [--no-undefined] [--always] (<commit>... | --all | --annotate-stdin)`
///
/// Finds a symbolic name for each revision, relative to the nearest ref that contains it:
/// `main`, `main~3`, `tags/v1^0`, `main~3^2~1`. Tags are preferred over branches and older
/// tags over newer ones. Prints `<argument> <name>`, or only the name with `--name-only`;
/// a revision without a name prints `undefined`; with `--no-undefined` that is an error,
/// unless `--always` is given too, which prints the abbreviated hash instead. `--tags` uses only tags (and with `--name-only` drops the
/// `tags/` prefix), `--refs` / `--exclude` restrict the refs by glob pattern, matched against
/// the full ref name or any trailing part of it. `--all` lists every commit reachable from
/// the refs with its name (sorted by hash), and `--annotate-stdin` copies standard input,
/// adding ` (name)` after every full hash it can name. Exit status: 0, 128 for a fatal
/// error, 129 for a usage error.
pub fn name_rev(args: Vec<String>) -> Result<()> {
    let opts = match parse_name_rev_args(&args) {
        Ok(opts) => opts,
        Err(message) => {
            if message.starts_with("usage:") {
                print!("{}", message);
            } else {
                eprint!("{}", message);
            }
            crate::ui::exit(129);
        }
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    if [opts.all, opts.annotate_stdin, !opts.revs.is_empty()].iter().filter(|on| **on).count() > 1 {
        eprint!("error: Specify either a list, or --all, not both!\n{}", NAME_REV_USAGE);
        crate::ui::exit(129);
    }

    // Resolve the arguments first: the oldest commit among them limits how far back names go.
    let mut cutoff = i64::MAX;
    let mut targets: Vec<(String, String, bool)> = Vec::new();
    for rev in &opts.revs {
        let Ok(hash) = resolve_rev(rev) else {
            eprintln!("Could not get sha1 for {}. Skipping.", rev);
            continue;
        };
        let Some((peeled, object_type, _, _)) = peel_ref_tip(&hash) else {
            eprintln!("Could not get object for {}. Skipping.", rev);
            continue;
        };
        let is_commit = crate::core::object_store::read_object(&hash).map(|(t, _)| t == "commit").unwrap_or(false);
        if object_type == "commit" {
            if let Some((_, date)) = commit_parents_and_date(&peeled) {
                cutoff = cutoff.min(date);
            }
        }
        targets.push((rev.clone(), hash, is_commit));
    }
    if opts.all || opts.annotate_stdin {
        cutoff = 0;
    }
    if cutoff != 0 {
        cutoff = cutoff.saturating_sub(CUTOFF_DATE_SLOP);
    }

    let names = build_rev_names(&opts, cutoff);
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if opts.annotate_stdin {
        let mut line = String::new();
        let stdin = io::stdin();
        let mut input = stdin.lock();
        loop {
            line.clear();
            if input.read_line(&mut line)? == 0 {
                break;
            }
            out.write_all(annotate_line(&names, &line, opts.name_only).as_bytes())?;
        }
        out.flush()?;
        return Ok(());
    }

    if opts.all {
        let mut commits: Vec<&String> = names.commits.keys().collect();
        commits.sort();
        targets = commits.into_iter().map(|hash| (hash.clone(), hash.clone(), true)).collect();
    }
    for (label, hash, is_commit) in &targets {
        // Like git: `undefined` unless --no-undefined, then the hash with --always, else an error.
        let name = match object_name(&names, hash, *is_commit) {
            Some(name) => name,
            None if !opts.no_undefined => "undefined".to_string(),
            None if opts.always => abbreviate_hash(hash),
            None => {
                if !opts.name_only {
                    write!(out, "{} ", label)?;
                }
                out.flush()?;
                eprintln!("fatal: cannot describe '{}'", hash);
                crate::ui::exit(128);
            }
        };
        if opts.name_only {
            writeln!(out, "{}", name)?;
        } else {
            writeln!(out, "{} {}", label, name)?;
        }
    }
    out.flush()?;
    Ok(())
}
