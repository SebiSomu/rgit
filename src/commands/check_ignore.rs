use std::collections::{BTreeSet, HashMap};
use std::io::{self, BufRead, BufWriter, Write};
use anyhow::Result;

use crate::helpers::check_ignore::{find_ignore_match, is_tracked_or_contains_tracked, normalize_check_ignore_path};
use crate::index::read_index;
use crate::objects::{CheckIgnoreOptions, SourcedIgnoreRule};

/// `rgit check-ignore [-q] [-v [-n]] [--no-index] [--stdin [-z] | <pathname>...]`
///
/// For each path, reports whether `.gitignore` rules exclude it. Plain mode prints each ignored
/// path; `-v` prints `<source>:<line>:<pattern>\t<path>` for the deciding rule (including
/// negated `!` rules, which also count as a match for the exit status, as in git), and `-n`
/// adds `::\t<path>` lines for paths no rule matched. Paths inside an ignored directory are
/// reported with the directory's rule. Tracked paths (and directories containing tracked files)
/// are skipped unless `--no-index` is given. Exits 0 if at least one path was ignored, 1 if
/// none were, and 128 on usage errors, so scripts can tell "not ignored" apart from a failure.
pub fn check_ignore(opts: CheckIgnoreOptions) -> Result<()> {
    match run_check_ignore(&opts) {
        Ok(0) => std::process::exit(1),
        Ok(_) => Ok(()),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(128);
        }
    }
}

fn run_check_ignore(opts: &CheckIgnoreOptions) -> Result<usize> {
    if opts.stdin && !opts.paths.is_empty() {
        anyhow::bail!("fatal: cannot specify pathnames with --stdin");
    }
    if opts.null_terminated && !opts.stdin {
        anyhow::bail!("fatal: -z only makes sense with --stdin");
    }
    if !opts.stdin && opts.paths.is_empty() {
        anyhow::bail!("fatal: no path specified");
    }
    if opts.quiet && opts.paths.len() > 1 {
        anyhow::bail!("fatal: --quiet is only valid with a single pathname");
    }
    if opts.quiet && opts.verbose {
        anyhow::bail!("fatal: cannot have both --quiet and --verbose");
    }
    if opts.non_matching && !opts.verbose {
        anyhow::bail!("fatal: --non-matching is only valid with --verbose");
    }

    let tracked: BTreeSet<String> = if opts.no_index {
        BTreeSet::new()
    } else {
        read_index()?.into_iter().map(|e| e.path).collect()
    };

    let mut cache = HashMap::new();
    let mut num_ignored = 0;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    if opts.stdin {
        let separator = if opts.null_terminated { b'\0' } else { b'\n' };
        let mut input = io::stdin().lock();
        let mut buf = Vec::new();
        loop {
            buf.clear();
            if input.read_until(separator, &mut buf)? == 0 {
                break;
            }
            if buf.last() == Some(&separator) {
                buf.pop();
            }
            if !opts.null_terminated && buf.last() == Some(&b'\r') {
                buf.pop();
            }
            let arg = String::from_utf8_lossy(&buf).to_string();
            if check_path(&arg, opts, &tracked, &mut cache, &mut out)? {
                num_ignored += 1;
            }
            out.flush()?;
        }
    } else {
        for arg in &opts.paths {
            if check_path(arg, opts, &tracked, &mut cache, &mut out)? {
                num_ignored += 1;
            }
        }
    }

    out.flush()?;
    Ok(num_ignored)
}

fn check_path(arg: &str, opts: &CheckIgnoreOptions, tracked: &BTreeSet<String>, cache: &mut HashMap<String, Vec<SourcedIgnoreRule>>, out: &mut impl Write) -> Result<bool> {
    let (path, is_dir) = normalize_check_ignore_path(arg)?;

    let mut found = None;
    if !path.is_empty() && !is_tracked_or_contains_tracked(&path, tracked) {
        found = find_ignore_match(&path, is_dir, cache);
    }
    if !opts.verbose && found.as_ref().is_some_and(|m| m.rule.negated) {
        found = None;
    }

    if !opts.quiet && (found.is_some() || opts.non_matching) {
        write_result(out, arg, found.as_ref(), opts)?;
    }
    Ok(found.is_some())
}

fn write_result(out: &mut impl Write, arg: &str, found: Option<&SourcedIgnoreRule>, opts: &CheckIgnoreOptions) -> io::Result<()> {
    let z = opts.null_terminated;
    if !opts.verbose {
        return write!(out, "{}{}", arg, if z { '\0' } else { '\n' });
    }
    match (found, z) {
        (Some(m), false) => writeln!(out, "{}:{}:{}\t{}", m.source, m.line_number, m.pattern_text, arg),
        (Some(m), true) => write!(out, "{}\0{}\0{}\0{}\0", m.source, m.line_number, m.pattern_text, arg),
        (None, false) => writeln!(out, "::\t{}", arg),
        (None, true) => write!(out, "\0\0\0{}\0", arg),
    }
}
