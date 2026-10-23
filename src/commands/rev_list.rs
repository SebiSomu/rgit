use std::io::{self, BufWriter, Write};
use anyhow::Result;

use crate::helpers::rev_list::{
    abbreviate_hash, build_commit_filter, expand_rev_list_format, parse_rev_list_args, resolve_rev_list_tips, rewrite_rev_list_parents,
    run_rev_list_walk, sort_rev_list_topologically,
};
use crate::helpers::format_patch::split_commit_message;
use crate::objects::{ParsedCommit, RevListFormat, RevListOptions, WalkedCommit};

/// `rgit rev-list [<options>] <commit>... [--] [<path>...]`
///
/// Lists commits reachable from the given revisions, newest first by committer date, excluding
/// those reachable from `^rev` / the left of `A..B` (or the merge bases of `A...B`). Supports
/// `--all`/`--branches`/`--tags`/`--remotes`, `--not`, `--topo-order`/`--date-order`,
/// `-n`/`--skip`/`--reverse`/`--count`, `--first-parent`, `--merges`/`--no-merges`/
/// `--min-parents`/`--max-parents`, `--since`/`--until`, `--author`/`--committer`/`--grep`, path
/// limiting with git's history simplification, and output with `--parents`, `--timestamp`,
/// `--left-right`, `--abbrev-commit`, `--oneline`, and `--format`. Exits 129 on usage errors
/// and 128 on bad revisions.
pub fn rev_list(args: Vec<String>) -> Result<()> {
    let opts = match parse_rev_list_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            eprintln!("{}", message);
            std::process::exit(if message.starts_with("usage:") { 129 } else { 128 });
        }
    };
    if let Err(e) = run_rev_list(&opts) {
        eprintln!("{}", e);
        std::process::exit(128);
    }
    Ok(())
}

fn run_rev_list(opts: &RevListOptions) -> Result<()> {
    let matches = build_commit_filter(opts)?;
    let shown = |c: &ParsedCommit, parent_count: usize| -> bool {
        opts.min_parents.is_none_or(|min| parent_count >= min)
            && opts.max_parents.is_none_or(|max| parent_count <= max)
            && opts.until.is_none_or(|until| c.committer.timestamp < until)
            && matches(c)
    };

    let roots = resolve_rev_list_tips(opts)?;
    let (mut walked, state) = run_rev_list_walk(opts, &roots, &shown)?;
    if opts.topo_order || opts.date_order {
        walked = sort_rev_list_topologically(walked, opts.date_order);
    }

    let shown: Vec<&WalkedCommit> = walked
        .iter()
        .filter(|w| !w.hidden && !state.uninteresting.contains(&w.commit.hash))
        .filter(|w| shown(&w.commit, w.parents.len()))
        .skip(opts.skip)
        .take(opts.max_count.unwrap_or(usize::MAX))
        .collect();

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if opts.count {
        if opts.left_right {
            let left = shown.iter().filter(|w| w.left).count();
            writeln!(out, "{}\t{}", left, shown.len() - left)?;
        } else {
            writeln!(out, "{}", shown.len())?;
        }
        return Ok(out.flush()?);
    }

    let mut entries: Vec<&WalkedCommit> = shown;
    if opts.reverse {
        entries.reverse();
    }

    for w in entries {
        let parents = if opts.paths.is_empty() {
            w.commit.parents.clone()
        } else {
            rewrite_rev_list_parents(&w.commit.parents, &state, opts.first_parent)
        };
        let show = |h: &str| if opts.abbrev_commit { abbreviate_hash(h) } else { h.to_string() };

        let mut line = String::new();
        if opts.timestamp {
            line.push_str(&format!("{} ", w.commit.committer.timestamp));
        }
        if matches!(opts.format, Some(RevListFormat::Template { .. })) {
            line.push_str("commit ");
        }
        if opts.left_right {
            line.push(if w.left { '<' } else { '>' });
        }
        line.push_str(&show(&w.commit.hash));
        if opts.parents {
            for p in &parents {
                line.push(' ');
                line.push_str(p);
            }
        }

        match &opts.format {
            None => writeln!(out, "{}", line)?,
            Some(RevListFormat::Oneline) => writeln!(out, "{} {}", line, split_commit_message(&w.commit.message).0)?,
            Some(RevListFormat::Template { template, .. }) => {
                writeln!(out, "{}", line)?;
                writeln!(out, "{}", expand_rev_list_format(template, &w.commit, &parents))?;
            }
        }
    }
    Ok(out.flush()?)
}
