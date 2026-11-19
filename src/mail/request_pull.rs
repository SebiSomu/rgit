pub mod engine;

use std::io::{self, Write};
use anyhow::Result;

use crate::merge::fmt_merge_msg::engine::read_branch_description;
use crate::core::commit::parse_commit_object;
use crate::diff::hunks::format_diffstat;
use crate::remote::github::parse_github_remote;
use crate::mail::mailmap::read_mailmap;
use crate::merge::merge::engine::find_merge_base;
use crate::mail::request_pull::engine::{
    diff_commit_trees, find_matching_remote_ref, format_range_shortlog, list_remote_refs_for_pull, parse_request_pull_args, remote_display_url, resolve_pull_head,
    tag_message_for_pull, REQUEST_PULL_USAGE,
};
use crate::history::rev_list::engine::expand_rev_list_format;
use crate::core::rev_parse::{peel_to_type, resolve_rev};
use crate::objects::RequestPullOptions;
use crate::core::repo;

const SEPARATOR: &str = "----------------------------------------------------------------";

/// `rgit request-pull [-p] <start> <url> [<end>]`
///
/// Writes a summary asking someone to pull your changes: the commit the work is based on
/// (the merge base of `<start>` and `<end>`), the repository `<url>` and ref to pull, the
/// tip commit, the message of the tag (when `<end>` is an annotated tag) or the branch
/// description (`branch.<name>.description`), a shortlog of the commits in
/// `<start>..<end>` grouped by author (honouring `.mailmap`), and a diffstat with
/// file creations, deletions and mode changes; `-p` adds the full patch. `<end>` defaults
/// to `HEAD` and may be `<local>:<remote>` when the ref has another name in the remote
/// repository. `<url>` (a path, a GitHub URL, or a configured remote) is checked for a ref
/// named like `<end>` that points at the same commit; if there is none, a warning is
/// printed and the exit status is 1, but the summary is still written. Renames are shown
/// as a deletion plus a creation, since rgit has no rename detection. Exit status, like
/// git: 0 success, 1 a warning or an invalid revision, 128 outside a repository.
pub fn request_pull(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        print!("{}", REQUEST_PULL_USAGE);
        return Ok(());
    }
    let opts = match parse_request_pull_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            if message.starts_with("usage:") {
                print!("{}", message);
                crate::ui::exit(1);
            }
            eprint!("{}", message);
            crate::ui::exit(129);
        }
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    match write_pull_request(&opts) {
        Ok(0) => Ok(()),
        Ok(status) => crate::ui::exit(status),
        Err(e) => {
            eprintln!("{}", e);
            crate::ui::exit(1);
        }
    }
}

/// Port of git-request-pull.sh. Returns the exit status (1 after a warning).
fn write_pull_request(opts: &RequestPullOptions) -> Result<i32> {
    let mut status = 0;
    let base_rev = resolve_rev(&opts.start).and_then(|hash| peel_to_type(hash, "commit")).map_err(|_| anyhow::anyhow!("fatal: Not a valid revision: {}", opts.start))?;

    // <end> is `<local>[:<remote>]`; without a colon both halves are the same name.
    let (local, remote) = match opts.end.rsplit_once(':') {
        Some((local, _)) => (local, opts.end.split_once(':').map(|(_, remote)| remote).unwrap_or("")),
        None => (opts.end.as_str(), opts.end.as_str()),
    };
    let local = if local.is_empty() { "HEAD" } else { local };
    let mut pretty_remote = remote.strip_prefix("refs/").unwrap_or(remote);
    pretty_remote = pretty_remote.strip_prefix("heads/").unwrap_or(pretty_remote);
    let mut pretty_remote = pretty_remote.to_string();
    let remote_name = if remote.is_empty() { "HEAD" } else { remote };

    let head = resolve_pull_head(local).ok_or_else(|| anyhow::anyhow!("fatal: Not a valid revision: {}", local))?;
    let head_rev = resolve_rev(&head).and_then(|hash| peel_to_type(hash, "commit")).map_err(|_| anyhow::anyhow!("fatal: Ambiguous revision: {}", local))?;
    let local_hash = resolve_rev(&head).unwrap_or_default();

    let branch_name = head.strip_prefix("refs/heads/").unwrap_or(&head);
    let description = if branch_name.is_empty() { None } else { read_branch_description(branch_name) };

    let merge_base = find_merge_base(&base_rev, &head_rev)?.ok_or_else(|| anyhow::anyhow!("fatal: No commits in common between {} and {}", opts.start, head))?;

    let remote_refs = list_remote_refs_for_pull(&opts.url).unwrap_or_else(|e| {
        if parse_github_remote(&remote_display_url(&opts.url)).is_some() {
            eprintln!("{}", e);
        } else {
            eprintln!("fatal: '{}' does not appear to be a git repository", opts.url);
        }
        eprintln!("fatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.");
        Vec::new()
    });
    match find_matching_remote_ref(&remote_refs, remote_name, &head_rev) {
        None => {
            eprintln!("warn: No match for commit {} found at {}", head_rev, opts.url);
            eprintln!("warn: Are you sure you pushed '{}' there?", remote_name);
            status = 1;
        }
        Some((remote_hash, found)) => {
            if local_hash != remote_hash {
                eprintln!("warn: {} found at {} but points to a different object", head, opts.url);
                eprintln!("warn: Are you sure you pushed '{}' there?", remote_name);
                status = 1;
            }
            // Turn "for_linus" into "tags/for_linus" when that is what it is.
            if found == format!("refs/tags/{}", pretty_remote) {
                pretty_remote = format!("tags/{}", pretty_remote);
            }
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let base_commit = parse_commit_object(&merge_base)?;
    let head_commit = parse_commit_object(&head_rev)?;
    writeln!(
        out,
        "{}",
        expand_rev_list_format("The following changes since commit %H:\n\n  %s (%ci)\n\nare available in the Git repository at:\n", &base_commit, &base_commit.parents)
    )?;
    writeln!(out, "  {} {}", remote_display_url(&opts.url), pretty_remote)?;
    writeln!(out, "{}", expand_rev_list_format(&format!("\nfor you to fetch changes up to %H:\n\n  %s (%ci)\n\n{}", SEPARATOR), &head_commit, &head_commit.parents))?;

    if let Some(message) = tag_message_for_pull(&head) {
        write!(out, "{}", message)?;
        writeln!(out, "\n{}", SEPARATOR)?;
    }
    if let Some(description) = description {
        writeln!(out, "(from the branch description for {} local branch)\n", branch_name)?;
        writeln!(out, "{}", description)?;
        writeln!(out, "{}", SEPARATOR)?;
    }

    write!(out, "{}", format_range_shortlog(&base_rev, &head_rev, &read_mailmap(None, None))?)?;

    let diffs = diff_commit_trees(&merge_base, &head_rev)?;
    if !diffs.is_empty() {
        let stats: Vec<_> = diffs.iter().map(|d| d.stat.clone()).collect();
        write!(out, "{}", format_diffstat(&stats, 80))?;
        for diff in &diffs {
            if let Some(summary) = &diff.summary {
                write!(out, "{}", summary)?;
            }
        }
        if opts.patch {
            writeln!(out)?;
            for diff in &diffs {
                write!(out, "{}", diff.text)?;
            }
        }
    }
    out.flush()?;
    Ok(status)
}
