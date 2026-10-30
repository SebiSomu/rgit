use std::collections::HashMap;
use std::io::{self, BufRead, BufWriter, Write};
use anyhow::Result;

use crate::helpers::for_each_ref::{
    collect_ref_items, compare_ref_items, filter_ref_items, format_ref_item, parse_for_each_ref_args, populate_ref_values, prepare_ref_format,
    ref_iteration_prefixes, version_suffixes, want_ref_color, worktree_paths_by_branch,
};
use crate::objects::{ForEachRefExit, RefAtomKind};

/// `rgit for-each-ref [<options>] [<pattern>...]`
///
/// Lists refs under `refs/` (loose and packed) through a format of `%(atom)` placeholders
/// (default `%(objectname) %(objecttype)\t%(refname)`), sorted by `--sort` keys (default
/// `refname`). Patterns match path prefixes or globs; `--exclude`, `--points-at`,
/// `--merged`/`--no-merged` and `--contains`/`--no-contains` filter further, `--count` limits
/// the output, `-s`/`-p`/`--python`/`--tcl` quote values, `--color` keeps `%(color:...)`, and
/// `--stdin` reads the patterns from standard input. Supports git's atoms (ref names and
/// their `:short`/`:lstrip`/`:rstrip` forms, object fields, people and dates, subject, body,
/// trailers, upstream/push tracking, `HEAD`, `worktreepath`, `describe`, `ahead-behind`,
/// `align` and `if`/`then`/`else`/`end`). Exits 129 on usage errors and 128 on fatal ones.
pub fn for_each_ref(args: Vec<String>) -> Result<()> {
    if let Err(exit) = run_for_each_ref(&args) {
        let _ = io::stdout().flush();
        print!("{}", exit.stdout);
        let _ = io::stdout().flush();
        if !exit.stderr.is_empty() {
            eprintln!("{}", exit.stderr);
        }
        std::process::exit(exit.code);
    }
    Ok(())
}

fn fatal(message: String) -> ForEachRefExit {
    ForEachRefExit { code: 128, stdout: String::new(), stderr: format!("fatal: {}", message) }
}

fn run_for_each_ref(args: &[String]) -> Result<(), ForEachRefExit> {
    let mut opts = parse_for_each_ref_args(args)?;
    let use_color = want_ref_color(opts.color);
    let mut atoms = Vec::new();
    let (pieces, need_reset, sorts) = prepare_ref_format(&opts, use_color, &mut atoms)?;

    if opts.stdin {
        if !opts.patterns.is_empty() {
            return Err(fatal("unknown arguments supplied with --stdin".to_string()));
        }
        for line in io::stdin().lock().lines() {
            let line = line.map_err(|e| fatal(e.to_string()))?;
            opts.patterns.push(line.strip_suffix('\r').unwrap_or(&line).to_string());
        }
    }

    let prefixes = ref_iteration_prefixes(&opts);
    let items = filter_ref_items(collect_ref_items(prefixes.as_deref()), &opts);
    let worktrees = if atoms.iter().any(|a| a.kind == RefAtomKind::WorktreePath) { worktree_paths_by_branch() } else { HashMap::new() };
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        let values = populate_ref_values(&item, &atoms, &worktrees).map_err(fatal)?;
        rows.push((item, values));
    }
    let suffixes = if sorts.iter().any(|s| s.2) { version_suffixes() } else { Vec::new() };
    rows.sort_by(|a, b| compare_ref_items((&a.0, &a.1), (&b.0, &b.1), &sorts, &atoms, opts.ignore_case, &suffixes));

    let limit = if opts.count == 0 { rows.len() } else { (opts.count as usize).min(rows.len()) };
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for (_, values) in rows.iter().take(limit) {
        match format_ref_item(&pieces, &atoms, values, opts.quote, need_reset) {
            Ok(line) => {
                let written = out.write_all(&line).and_then(|_| if !line.is_empty() || !opts.omit_empty { out.write_all(b"\n") } else { Ok(()) });
                if written.is_err() {
                    return Ok(());
                }
            }
            Err(message) => {
                let _ = out.flush();
                return Err(fatal(message));
            }
        }
    }
    let _ = out.flush();
    Ok(())
}
