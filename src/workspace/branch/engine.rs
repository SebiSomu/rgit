use std::fs;
use std::path::Path;

use anyhow::Result;

use crate::core::config::{read_all_config_entries, read_entries_from, set_in, unset_in, write_entries_to, LOCAL_CONFIG_PATH};
use crate::references::{
    for_each_ref::engine::refs::{collect_ref_items, filter_ref_items},
    for_each_ref::engine::upstream::{branch_upstream, shorten_unambiguous_ref, tracking_counts},
};
use crate::core::object_store::read_object;
use crate::core::wildmatch::wildmatch;
use crate::references::for_each_ref::types::{ForEachRefOptions, RefArrayItem};
use crate::core::refs;
use crate::workspace::branch::types::{BranchListEntry, BranchOptions};

pub const BRANCH_USAGE: &str = "usage: rgit branch [<options>] [-r | -a] [--merged] [--no-merged]
   or: rgit branch [<options>] [-f] <branch-name> [<start-point>]
   or: rgit branch [<options>] [-l] [<pattern>...]
   or: rgit branch [<options>] [-r] (-d | -D) <branch-name>...
   or: rgit branch [<options>] (-m | -M) [<old-branch>] <new-branch>
   or: rgit branch [<options>] (-c | -C) [<old-branch>] <new-branch>
   or: rgit branch [<options>] [-r | -a] [--points-at]

    -v, --verbose         show hash and subject, give twice for upstream branch
    -q, --quiet           suppress informational messages
    -t, --track           set branch tracking configuration
    --no-track            do not set up tracking
    -u, --set-upstream-to <upstream>
                          change the upstream info
    --unset-upstream      unset the upstream info
    -r, --remotes         act on remote-tracking branches
    --contains <commit>   print only branches that contain the commit
    --no-contains <commit>
                          print only branches that don't contain the commit
    -a, --all             list both remote-tracking and local branches
    -d, --delete          delete fully merged branch
    -D                    delete branch (even if not merged)
    -m, --move            move/rename a branch and its reflog
    -M                    move/rename a branch, even if target exists
    -c, --copy            copy a branch and its reflog
    -C                    copy a branch, even if target exists
    -l, --list            list branch names
    --show-current        show current branch name
    -f, --force           force creation, move/rename, deletion
    --merged <commit>     print only branches that are merged
    --no-merged <commit>  print only branches that are not merged
    --points-at <object>  print only branches of the object
";

/// Prints a usage error (git's `error: ...` line plus the usage text) and exits 129.
// Used by `parse_branch_args`.
fn usage_error(message: &str) -> ! {
    eprintln!("error: {}", message);
    eprint!("{}", BRANCH_USAGE);
    crate::ui::exit(129);
}

/// Resolves a `--merged`/`--contains`/... argument to a commit, exiting 128 with git's
/// "malformed object name" when it names nothing (or nothing that peels to a commit).
// Used by `parse_branch_args`.
fn resolve_filter_commit(rev: &str) -> String {
    match crate::core::rev_parse::resolve_rev(&format!("{}^{{commit}}", rev)) {
        Ok(hash) => hash,
        Err(_) => {
            eprintln!("fatal: malformed object name {}", rev);
            crate::ui::exit(128);
        }
    }
}

/// Parses `rgit branch` arguments the way git's parse-options does: bundled short flags
/// (`-vv`, `-dr`), `--opt=value` / `--opt value`, `-u<up>`, `--` ending options, and the
/// filter options that take HEAD when they are the last argument.
// Used by the `branch` command.
pub fn parse_branch_args(args: &[String]) -> BranchOptions {
    let mut opts = BranchOptions::default();
    let mut i = 0;
    let mut only_args = false;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if only_args || arg == "-" || !arg.starts_with('-') {
            opts.args.push(arg.clone());
            continue;
        }
        if arg == "--" {
            only_args = true;
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            // The value of an option that requires one: inline, or the next argument.
            let take_value = |i: &mut usize| -> String {
                if let Some(v) = inline.clone() {
                    return v;
                }
                if *i < args.len() {
                    *i += 1;
                    return args[*i - 1].clone();
                }
                usage_error(&format!("option `{}' requires a value", name));
            };
            match name {
                "delete" => opts.delete = true,
                "move" => opts.rename = true,
                "copy" => opts.copy = true,
                "force" => opts.force = true,
                "remotes" => opts.remotes = true,
                "all" => opts.all = true,
                "list" => opts.list = true,
                "verbose" => opts.verbose += 1,
                "no-verbose" => opts.verbose = 0,
                "quiet" => opts.quiet = true,
                "track" => opts.track = Some(true),
                "no-track" => opts.track = Some(false),
                "set-upstream-to" => opts.set_upstream = Some(take_value(&mut i)),
                "unset-upstream" => opts.unset_upstream = true,
                "show-current" => opts.show_current = true,
                "points-at" => {
                    let v = take_value(&mut i);
                    match crate::core::rev_parse::resolve_rev(&v) {
                        Ok(hash) => opts.points_at.push(hash),
                        Err(_) => {
                            eprintln!("error: malformed object name '{}'", v);
                            crate::ui::exit(129);
                        }
                    }
                }
                "merged" | "no-merged" | "contains" | "no-contains" => {
                    // LASTARG_DEFAULT: HEAD when this is the last argument.
                    let rev = match inline.clone() {
                        Some(v) => v,
                        None if i < args.len() => {
                            i += 1;
                            args[i - 1].clone()
                        }
                        None => "HEAD".to_string(),
                    };
                    let commit = resolve_filter_commit(&rev);
                    match name {
                        "merged" => opts.merged.push(commit),
                        "no-merged" => opts.no_merged.push(commit),
                        "contains" => opts.contains.push(commit),
                        _ => opts.no_contains.push(commit),
                    }
                }
                _ => usage_error(&format!("unknown option `{}'", name)),
            }
            continue;
        }
        // Bundled short options.
        let letters: Vec<char> = arg[1..].chars().collect();
        let mut k = 0;
        while k < letters.len() {
            let c = letters[k];
            k += 1;
            match c {
                'd' => opts.delete = true,
                'D' => {
                    opts.delete = true;
                    opts.force = true;
                }
                'm' => opts.rename = true,
                'M' => {
                    opts.rename = true;
                    opts.force = true;
                }
                'c' => opts.copy = true,
                'C' => {
                    opts.copy = true;
                    opts.force = true;
                }
                'f' => opts.force = true,
                'r' => opts.remotes = true,
                'a' => opts.all = true,
                'l' => opts.list = true,
                'v' => opts.verbose += 1,
                'q' => opts.quiet = true,
                't' => opts.track = Some(true),
                'u' => {
                    let rest: String = letters[k..].iter().collect();
                    let value = if !rest.is_empty() {
                        rest
                    } else if i < args.len() {
                        i += 1;
                        args[i - 1].clone()
                    } else {
                        usage_error("switch `u' requires a value");
                    };
                    opts.set_upstream = Some(value);
                    k = letters.len();
                }
                other => usage_error(&format!("unknown switch `{}'", other)),
            }
        }
    }
    opts
}

/// The first paragraph of a commit message joined into one line, like `%(contents:subject)`.
// Used by `format_branch_list` (`branch -v`).
fn commit_subject(oid: &str) -> String {
    let Ok((kind, content)) = read_object(oid) else { return String::new() };
    if kind != "commit" {
        return String::new();
    }
    let text = String::from_utf8_lossy(&content);
    let Some((_, message)) = text.split_once("\n\n") else { return String::new() };
    let mut words = Vec::new();
    for line in message.lines() {
        if line.trim().is_empty() {
            if words.is_empty() {
                continue;
            }
            break;
        }
        words.push(line.trim_end().to_string());
    }
    words.join(" ")
}

/// Returns `refs/remotes/...` or `refs/heads/...` (the branch named `name`), if it exists.
// Used by `upstream_for`.
fn ref_if_exists(refname: &str) -> Option<String> {
    refs::read_ref(refname).ok().flatten().map(|_| refname.to_string())
}

/// Names of the remotes configured in `remote.<name>.*`.
// Used by `upstream_for`.
fn configured_remotes() -> Vec<String> {
    let mut names = Vec::new();
    for (key, _) in read_all_config_entries() {
        if let Some(rest) = key.strip_prefix("remote.") {
            if let Some((name, _)) = rest.rsplit_once('.') {
                if !names.iter().any(|n: &String| n == name) {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

/// Works out what tracking `start` names: a remote-tracking branch `<remote>/<b>` gives
/// (`<remote>`, `refs/heads/<b>`), a local branch gives (`.`, `refs/heads/<b>`). The third
/// value is the name git prints in "set up to track '<name>'".
// Used by `branch <name> <start>` (tracking) and `branch -u`.
pub fn upstream_for(start: &str) -> Option<(String, String, String)> {
    let short = start.strip_prefix("refs/remotes/").unwrap_or(start);
    if ref_if_exists(&format!("refs/remotes/{}", short)).is_some() {
        let remote = configured_remotes()
            .into_iter()
            .filter(|r| short.starts_with(&format!("{}/", r)))
            .max_by_key(|r| r.len())
            .or_else(|| short.split_once('/').map(|(r, _)| r.to_string()))?;
        let branch = &short[remote.len() + 1..];
        return Some((remote, format!("refs/heads/{}", branch), short.to_string()));
    }
    let local = start.strip_prefix("refs/heads/").unwrap_or(start);
    if ref_if_exists(&format!("refs/heads/{}", local)).is_some() {
        return Some((".".to_string(), format!("refs/heads/{}", local), local.to_string()));
    }
    None
}

/// Writes `branch.<branch>.remote` / `.merge` to the repository config.
// Used by `branch -u`, `branch --track` and branches created from a remote-tracking branch.
pub fn set_branch_upstream(branch: &str, remote: &str, merge: &str) -> Result<()> {
    let config = crate::core::repo::path(LOCAL_CONFIG_PATH);
    set_in(&config, &format!("branch.{}.remote", branch), remote)?;
    set_in(&config, &format!("branch.{}.merge", branch), merge)?;
    Ok(())
}

/// Removes `branch.<branch>.remote` / `.merge`; returns false when neither was set.
// Used by `branch --unset-upstream`.
pub fn unset_branch_upstream(branch: &str) -> Result<bool> {
    let config = crate::core::repo::path(LOCAL_CONFIG_PATH);
    let a = unset_in(&config, &format!("branch.{}.remote", branch))?;
    let b = unset_in(&config, &format!("branch.{}.merge", branch))?;
    Ok(a || b)
}

/// Renames (or, with `copy`, duplicates) the `[branch "<old>"]` config section to `<new>`;
/// with `new` = None the section is deleted.
// Used by `branch -m/-M`, `branch -c/-C` and `branch -d/-D`.
pub fn move_branch_config(old: &str, new: Option<&str>, copy: bool) -> Result<()> {
    let config = crate::core::repo::path(LOCAL_CONFIG_PATH);
    let entries = read_entries_from(&config)?;
    let old_prefix = format!("branch.{}.", old);
    if !entries.iter().any(|(k, _)| k.starts_with(&old_prefix)) {
        return Ok(());
    }
    let mut out = Vec::new();
    let mut copied = Vec::new();
    for (key, value) in entries {
        match key.strip_prefix(&old_prefix) {
            Some(name) => {
                if let Some(new) = new {
                    copied.push((format!("branch.{}.{}", new, name), value.clone()));
                }
                if copy {
                    out.push((key, value));
                }
            }
            None => out.push((key, value)),
        }
    }
    if let Some(new) = new {
        // A section already named after the target is replaced.
        let new_prefix = format!("branch.{}.", new);
        out.retain(|(k, _)| !k.starts_with(&new_prefix));
    }
    out.extend(copied);
    write_entries_to(&config, &out)
}

/// Deletes the remote-tracking ref `refs/remotes/<name>` (loose and packed) and its reflog.
// Used by `branch -d -r`.
pub fn delete_remote_tracking_ref(name: &str) -> Result<()> {
    let refname = format!("refs/remotes/{}", name);
    let path = crate::core::repo::path(format!(".git/{}", refname));
    if Path::new(&path).is_file() {
        fs::remove_file(&path)?;
        refs::prune_empty_ref_dirs(Path::new(&path));
    }
    refs::remove_packed_ref(&refname)?;
    crate::core::reflog::delete_reflog(&refname);
    Ok(())
}

/// The target of a symbolic ref (`ref: refs/remotes/origin/main`), if `refname` is one.
// Used by `format_branch_list` (`origin/HEAD -> origin/main`) and `branch -d -r`.
pub fn symref_target(refname: &str) -> Option<String> {
    let content = fs::read_to_string(crate::core::repo::path(format!(".git/{}", refname))).ok()?;
    content.trim().strip_prefix("ref: ").map(|s| s.to_string())
}

/// git's `%(upstream:track)` for a local branch: `[ahead N, behind M]`, `[gone]`, or empty
/// when in sync or without upstream. Returns (upstream short name, track text without brackets).
// Used by `format_branch_list` (`-v` / `-vv`).
fn upstream_track(branch: &str) -> Option<(String, String)> {
    let upstream = branch_upstream(branch)?;
    let track = match tracking_counts(&format!("refs/heads/{}", branch), &upstream) {
        None => "gone".to_string(),
        Some((0, 0)) => String::new(),
        Some((ahead, 0)) => format!("ahead {}", ahead),
        Some((0, behind)) => format!("behind {}", behind),
        Some((ahead, behind)) => format!("ahead {}, behind {}", ahead, behind),
    };
    Some((shorten_unambiguous_ref(&upstream), track))
}

/// Collects the branches `branch --list` shows (local, `-r` remote-tracking, `-a` both, plus a
/// detached HEAD), filtered by patterns and `--merged`/`--contains`/`--points-at`, and renders
/// them in git's default (or `-v` / `-vv`) format.
// Used by the `branch` command (listing mode).
pub fn format_branch_list(opts: &BranchOptions) -> Result<String> {
    let want_local = !opts.remotes || opts.all;
    let want_remote = opts.remotes || opts.all;
    let filter = ForEachRefOptions {
        merged: opts.merged.clone(),
        no_merged: opts.no_merged.clone(),
        contains: opts.contains.clone(),
        no_contains: opts.no_contains.clone(),
        points_at: opts.points_at.clone(),
        ..Default::default()
    };
    let matches = |short: &str| opts.args.is_empty() || opts.args.iter().any(|p| wildmatch(p, short, 0));

    let head = refs::resolve_head()?;
    let mut items: Vec<RefArrayItem> = Vec::new();
    if want_local {
        if let refs::HeadState::Detached(hash) = &head {
            items.push(RefArrayItem { refname: "HEAD".to_string(), oid: hash.clone(), symref: false, packed: false });
        }
    }
    let mut prefixes = Vec::new();
    if want_local {
        prefixes.push("refs/heads/".to_string());
    }
    if want_remote {
        prefixes.push("refs/remotes/".to_string());
    }
    items.extend(collect_ref_items(Some(&prefixes)).into_iter().filter(|i| prefixes.iter().any(|p| i.refname.starts_with(p))));
    let items = filter_ref_items(items, &filter);

    let current = match &head {
        refs::HeadState::Branch(b) => Some(format!("refs/heads/{}", b)),
        refs::HeadState::Detached(_) => None,
    };
    let mut entries = Vec::new();
    for item in items {
        let (name, short, kind) = if item.refname == "HEAD" {
            (format!("(HEAD detached at {})", &item.oid[..7]), "HEAD".to_string(), 0)
        } else if let Some(s) = item.refname.strip_prefix("refs/heads/") {
            (s.to_string(), s.to_string(), 1)
        } else {
            let s = item.refname.strip_prefix("refs/remotes/").unwrap_or(&item.refname).to_string();
            (if opts.all { format!("remotes/{}", s) } else { s.clone() }, s, 2)
        };
        if !matches(&short) {
            continue;
        }
        let other_worktree = if kind == 1 && current.as_deref() != Some(item.refname.as_str()) {
            crate::workspace::worktree::engine::worktree_using_branch(&item.refname, false)?.map(|w| w.display_path())
        } else {
            None
        };
        entries.push(BranchListEntry {
            current: kind == 0 || current.as_deref() == Some(item.refname.as_str()),
            symref: if kind == 2 { symref_target(&item.refname) } else { None },
            name,
            short,
            kind,
            oid: item.oid,
            other_worktree,
        });
    }
    entries.sort_by(|a, b| (a.kind.min(1), &a.kind, &a.short).cmp(&(b.kind.min(1), &b.kind, &b.short)));

    let width = entries.iter().map(|e| e.name.chars().count()).max().unwrap_or(0);
    let mut out = String::new();
    for e in &entries {
        let marker = if e.current { "* " } else if e.other_worktree.is_some() { "+ " } else { "  " };
        out.push_str(marker);
        let target_short = e.symref.as_deref().map(|t| t.strip_prefix("refs/remotes/").unwrap_or(t).to_string());
        if opts.verbose == 0 {
            out.push_str(&e.name);
            if let Some(t) = &target_short {
                out.push_str(&format!(" -> {}", t));
            }
            out.push('\n');
            continue;
        }
        out.push_str(&format!("{:<w$}", e.name, w = width));
        if let Some(t) = &target_short {
            out.push_str(&format!(" -> {}\n", t));
            continue;
        }
        out.push_str(&format!(" {} ", &e.oid[..7]));
        if e.kind == 1 {
            let tracking = upstream_track(&e.short);
            if opts.verbose > 1 {
                if let Some(path) = &e.other_worktree {
                    out.push_str(&format!("({}) ", path));
                }
                if let Some((up, track)) = &tracking {
                    if track.is_empty() {
                        out.push_str(&format!("[{}] ", up));
                    } else {
                        out.push_str(&format!("[{}: {}] ", up, track));
                    }
                }
            } else if let Some((_, track)) = &tracking {
                if !track.is_empty() {
                    out.push_str(&format!("[{}] ", track));
                }
            }
        }
        out.push_str(&commit_subject(&e.oid));
        out.push('\n');
    }
    Ok(out)
}

