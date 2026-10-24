use std::collections::HashSet;
use std::io::{self, BufWriter, Write};
use anyhow::Result;

use crate::helpers::cherry::{abbreviate_hash_min, commit_patch_id, resolve_cherry_upstream, walk_non_merge_commits};
use crate::helpers::format_patch::{resolve_commit_rev, split_commit_message};
use crate::objects::CherryOptions;

/// `rgit cherry [-v] [--abbrev[=<n>]] [<upstream> [<head> [<limit>]]]`
///
/// Finds the commits in `<head>` (default `HEAD`) that are not in `<upstream>` (default the
/// current branch's upstream) and marks each one `-` if an equivalent change (same patch ID)
/// already exists in `<upstream>`, or `+` if it hasn't been applied there yet. Commits are
/// listed oldest first; merges are ignored on both sides. With `<limit>`, commits reachable
/// from it are left out. `-v` adds each commit's subject, and `--abbrev[=<n>]` shortens the
/// hashes. Exits 129 when no upstream can be found and 128 on unknown commits.
pub fn cherry(opts: CherryOptions) -> Result<()> {
    let upstream_name = match opts.upstream.clone() {
        Some(u) => u,
        None => match resolve_cherry_upstream()? {
            Some(u) => u,
            None => {
                eprintln!("Could not find a tracked remote branch, please specify <upstream> manually.");
                eprintln!("usage: rgit cherry [-v] [--abbrev[=<n>]] [<upstream> [<head> [<limit>]]]");
                std::process::exit(129);
            }
        },
    };
    let resolve = |name: &str| -> String {
        resolve_commit_rev(name).unwrap_or_else(|_| {
            eprintln!("fatal: unknown commit {}", name);
            std::process::exit(128);
        })
    };
    let head = resolve(opts.head.as_deref().unwrap_or("HEAD"));
    let upstream = resolve(&upstream_name);
    if head == upstream {
        return Ok(());
    }
    let limit = opts.limit.as_deref().map(resolve);

    let mut upstream_ids = HashSet::new();
    for commit in walk_non_merge_commits(&upstream, &[&head])? {
        upstream_ids.insert(commit_patch_id(&commit)?);
    }

    let mut exclude = vec![upstream.as_str()];
    if let Some(limit) = &limit {
        exclude.push(limit);
    }
    let mut commits = walk_non_merge_commits(&head, &exclude)?;
    commits.reverse();

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    for commit in commits {
        let sign = if upstream_ids.contains(&commit_patch_id(&commit)?) { '-' } else { '+' };
        let hash = match opts.abbrev {
            Some(len) => abbreviate_hash_min(&commit.hash, len),
            None => commit.hash.clone(),
        };
        if opts.verbose {
            writeln!(out, "{} {} {}", sign, hash, split_commit_message(&commit.message).0)?;
        } else {
            writeln!(out, "{} {}", sign, hash)?;
        }
    }
    Ok(out.flush()?)
}
