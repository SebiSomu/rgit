use std::collections::{HashMap, HashSet};
use crate::helpers::for_each_ref::versioncmp;
use crate::helpers::format_patch::{parse_commit_object, split_commit_message};
use crate::helpers::rev_list::list_all_refs;
use crate::core::rev_parse::{peel_to_type, resolve_rev};
use crate::helpers::symbolic_ref::resolve_ref_unsafe;
use crate::core::wildmatch::wildmatch;
use crate::objects::{ParsedCommit, ShowBranchOptions};

/// git's usage text for `show-branch`, printed for `-h` and after usage errors (exit 129).
pub const SHOW_BRANCH_USAGE: &str = "usage: rgit show-branch [-a | --all] [-r | --remotes] [--topo-order | --date-order]
                        [--current] [--more=<n> | --list | --independent | --merge-base]
                        [--no-name | --sha1-name] [--topics] [--sparse]
                        [(<rev> | <glob>)...]
";

/// The flag bit marking a commit as reachable from every tip's merge base (git's `UNINTERESTING`).
const UNINTERESTING: u32 = 1;
/// Tip number `i` uses flag bit `REV_SHIFT + i`.
const REV_SHIFT: u32 = 2;
/// Most tips `show-branch` can handle (git's `MAX_REVS`).
const MAX_REVS: usize = 26;

/// Parses `show-branch` arguments like git: the mode and display switches, `--more=<n>`
/// (`--more` alone is 1, `--list` is -1), and revisions or ref globs. With no arguments at
/// all, the `showbranch.default` configuration supplies them. Errors carry the usage text
/// and mean exit 129.
// Used by `show-branch`.
pub fn parse_show_branch_args(args: &[String]) -> anyhow::Result<ShowBranchOptions> {
    let defaults: Vec<String>;
    let args = if args.is_empty() {
        defaults = crate::core::config::read_all_config_entries().into_iter().filter(|(key, _)| key == "showbranch.default").map(|(_, value)| value).collect();
        &defaults[..]
    } else {
        args
    };

    let mut opts = ShowBranchOptions::default();
    let mut only_revs = false;
    for arg in args {
        if only_revs || !arg.starts_with('-') {
            opts.revs.push(arg.clone());
            continue;
        }
        match arg.as_str() {
            "--" => only_revs = true,
            "-a" | "--all" => opts.all_heads = true,
            "-r" | "--remotes" => opts.all_remotes = true,
            "--more" => opts.extra = 1,
            "--list" => opts.extra = -1,
            "--no-name" => opts.no_name = true,
            "--sha1-name" => opts.sha1_name = true,
            "--current" => opts.current = true,
            "--merge-base" => opts.merge_base = true,
            "--independent" => opts.independent = true,
            "--topo-order" => opts.date_order = false,
            "--date-order" => opts.date_order = true,
            "--topics" => opts.topics = true,
            "--sparse" => opts.sparse = true,
            "--color" | "--no-color" => {}
            "-g" | "--reflog" => anyhow::bail!("fatal: --reflog is not supported by rgit\n"),
            _ => match arg.strip_prefix("--more=").map(|n| n.parse::<i64>()) {
                Some(Ok(n)) => opts.extra = n,
                Some(Err(_)) => anyhow::bail!("error: option `more' expects a numerical value\n"),
                None if arg.starts_with("--color=") => {}
                None if arg.starts_with("--") => anyhow::bail!("error: unknown option `{}'\n{}", &arg[2..], SHOW_BRANCH_USAGE),
                None => anyhow::bail!("error: unknown switch `{}'\n{}", &arg[1..2], SHOW_BRANCH_USAGE),
            },
        }
    }
    if opts.all_heads {
        opts.all_remotes = true;
    }
    if opts.extra != 0 && (opts.independent || opts.merge_base) {
        anyhow::bail!("{}", SHOW_BRANCH_USAGE);
    }
    if opts.revs.len() <= usize::from(opts.topics) && !opts.all_heads && !opts.all_remotes {
        opts.all_heads = true;
    }
    Ok(opts)
}

/// git's `rev_is_head`: `name` (as listed) is the branch HEAD points at.
// Used by `collect_show_branch_names` and `show-branch` (the `*` markers).
pub fn rev_is_head(head: Option<&str>, name: &str) -> bool {
    let Some(head) = head else { return false };
    let head = head.strip_prefix("refs/heads/").unwrap_or(head);
    let name = name.strip_prefix("refs/heads/").or_else(|| name.strip_prefix("heads/")).unwrap_or(name);
    head == name
}

/// Resolves a revision, also accepting git's `heads/<branch>` and `tags/<tag>` spellings
/// (which `show-branch` itself prints when a short name would be ambiguous).
// Used by `show-branch` (internal to this file's helpers).
fn resolve_name(name: &str) -> anyhow::Result<String> {
    resolve_rev(name).or_else(|e| if name.starts_with("heads/") || name.starts_with("tags/") || name.starts_with("remotes/") { resolve_rev(&format!("refs/{}", name)) } else { Err(e) })
}

/// Resolves a name to the commit it points at (peeling tags); `None` for anything else.
// Used by `show-branch` (internal to this file's helpers).
fn commit_of(name: &str) -> Option<String> {
    resolve_name(name).ok().and_then(|hash| peel_to_type(hash, "commit").ok())
}

/// Ports of git's `append_one_rev`, `snarf_refs` and the `--current` handling: turns the
/// command line into the list of tip names to show. A name that isn't a revision but has
/// glob characters adds every matching ref; `--all` / `--remotes` add the branches and
/// remote-tracking branches (shortened to `foo`, or `heads/foo` when that would be
/// ambiguous), each group in version order; `--current` adds the current branch. Returns
/// the names and the ref HEAD points at.
// Used by `show-branch`.
pub fn collect_show_branch_names(opts: &ShowBranchOptions) -> anyhow::Result<(Vec<String>, Option<String>)> {
    let refs = list_all_refs();
    let mut names: Vec<String> = Vec::new();
    let append = |names: &mut Vec<String>, name: &str, hash: &str| {
        if peel_to_type(hash.to_string(), "commit").is_err() || names.iter().any(|n| n == name) {
            return;
        }
        if names.len() >= MAX_REVS {
            eprintln!("warning: ignoring {}; cannot handle more than {} refs", name, MAX_REVS);
            return;
        }
        names.push(name.to_string());
    };
    // `refs/heads/foo` is listed as `foo` unless that name would resolve to something else.
    let shortened = |full: &str, prefix_len: usize, hash: &str| -> String {
        let short = &full[prefix_len..];
        if resolve_name(short).is_ok_and(|resolved| resolved == hash) { short.to_string() } else { full[5..].to_string() }
    };
    let sort_range = |names: &mut Vec<String>, from: usize| names[from..].sort_by(|a, b| versioncmp(a.as_bytes(), b.as_bytes(), &[]).cmp(&0));

    for rev in &opts.revs {
        if let Ok(hash) = resolve_name(rev) {
            append(&mut names, rev, &hash);
            continue;
        }
        if !rev.contains(['*', '?', '[']) {
            anyhow::bail!("fatal: bad sha1 reference {}", rev);
        }
        let saved = names.len();
        let pattern_slashes = rev.matches('/').count();
        for (full, hash) in &refs {
            // Compare the pattern against as many trailing path components as it has itself.
            let mut slashes = full.matches('/').count();
            let mut tail = full.as_str();
            while !tail.is_empty() && pattern_slashes < slashes {
                let cut = tail.find('/').map(|p| p + 1).unwrap_or(tail.len());
                tail = &tail[cut..];
                slashes -= 1;
            }
            if tail.is_empty() || !wildmatch(rev, tail, 0) {
                continue;
            }
            if full.starts_with("refs/heads/") {
                append(&mut names, &shortened(full, 11, hash), hash);
            } else if full.starts_with("refs/tags/") {
                append(&mut names, &full[5..], hash);
            } else {
                append(&mut names, full, hash);
            }
        }
        if saved == names.len() && names.len() < MAX_REVS {
            eprintln!("error: no matching refs with {}", rev);
        }
        sort_range(&mut names, saved);
    }

    for (wanted, prefix) in [(opts.all_heads, "refs/heads/"), (opts.all_remotes, "refs/remotes/")] {
        if !wanted {
            continue;
        }
        let saved = names.len();
        for (full, hash) in refs.iter().filter(|(full, _)| full.starts_with(prefix)) {
            append(&mut names, &shortened(full, prefix.len(), hash), hash);
        }
        sort_range(&mut names, saved);
    }

    let head = resolve_ref_unsafe("HEAD", true, true).filter(|r| r.hash.is_some()).map(|r| r.name);
    if opts.current {
        if let Some(head_ref) = &head {
            if !names.iter().any(|name| rev_is_head(Some(head_ref), name)) {
                let short = head_ref.strip_prefix("refs/heads/").unwrap_or(head_ref);
                if let Ok(hash) = resolve_name(short) {
                    append(&mut names, short, &hash);
                }
            }
        }
    }
    Ok((names, head))
}

/// The commit graph `show-branch` works on: parsed commits, their flag bits (bit 0
/// "uninteresting", bit `2 + i` "reachable from tip i"), and the commits seen so far.
// Used by `show-branch` (internal to this file's helpers).
struct Graph {
    commits: HashMap<String, ParsedCommit>,
    flags: HashMap<String, u32>,
    seen: Vec<String>,
}

impl Graph {
    fn load(&mut self, hash: &str) -> anyhow::Result<()> {
        if !self.commits.contains_key(hash) {
            self.commits.insert(hash.to_string(), parse_commit_object(hash)?);
        }
        Ok(())
    }
    fn flag(&self, hash: &str) -> u32 {
        self.flags.get(hash).copied().unwrap_or(0)
    }
    fn date(&self, hash: &str) -> i64 {
        self.commits.get(hash).map(|c| c.committer.timestamp).unwrap_or(0)
    }
    /// git's `mark_seen`: a commit joins the seen list the first time it is met unflagged.
    fn mark_seen(&mut self, hash: &str) -> bool {
        if self.flag(hash) == 0 && !self.seen.iter().any(|s| s == hash) {
            self.seen.push(hash.to_string());
            return true;
        }
        false
    }
    /// git's `commit_list_insert_by_date`: after every queued commit that is not older.
    fn insert_by_date(&self, queue: &mut Vec<String>, hash: &str) {
        let date = self.date(hash);
        let at = queue.iter().position(|q| self.date(q) < date).unwrap_or(queue.len());
        queue.insert(at, hash.to_string());
    }
}

/// Port of git's `join_revs`: walks from all tips at once, newest commit first, pushing
/// each commit's tip flags down to its parents until everything still queued is reachable
/// from a commit that all tips share (plus `extra` more commits); then marks everything
/// reachable from such merge bases as uninteresting.
// Used by `show_branch_output`.
fn join_revs(graph: &mut Graph, queue: &mut Vec<String>, num_rev: usize, mut extra: i64) -> anyhow::Result<()> {
    let all_mask: u32 = (1u32 << (REV_SHIFT + num_rev as u32)) - 1;
    let all_revs = all_mask & !((1u32 << REV_SHIFT) - 1);

    while !queue.is_empty() {
        let still_interesting = queue.iter().any(|hash| graph.flag(hash) & UNINTERESTING == 0);
        let commit = queue.remove(0);
        let mut flags = graph.flag(&commit) & all_mask;
        if !still_interesting && extra <= 0 {
            break;
        }
        graph.mark_seen(&commit);
        if flags & all_revs == all_revs {
            flags |= UNINTERESTING;
        }
        for parent in graph.commits[&commit].parents.clone() {
            if graph.flag(&parent) & flags == flags {
                continue;
            }
            graph.load(&parent)?;
            if graph.mark_seen(&parent) && !still_interesting {
                extra -= 1;
            }
            *graph.flags.entry(parent.clone()).or_insert(0) |= flags;
            graph.insert_by_date(queue, &parent);
        }
    }

    loop {
        let mut changed = false;
        for hash in graph.seen.clone() {
            let flag = graph.flag(&hash);
            if flag & all_revs != all_revs && flag & UNINTERESTING == 0 {
                continue;
            }
            for parent in graph.commits[&hash].parents.clone() {
                if graph.flag(&parent) & UNINTERESTING == 0 {
                    *graph.flags.entry(parent).or_insert(0) |= UNINTERESTING;
                    changed = true;
                }
            }
        }
        if !changed {
            return Ok(());
        }
    }
}

/// Port of git's `sort_in_topological_order` on the seen list: children before parents,
/// keeping the incoming order among unrelated commits (or newest first with `date_order`).
// Used by `show_branch_output`.
fn sort_topologically(graph: &Graph, list: Vec<String>, date_order: bool) -> Vec<String> {
    let in_list: HashSet<&String> = list.iter().collect();
    let mut indegree: HashMap<&String, usize> = list.iter().map(|hash| (hash, 1)).collect();
    for hash in &list {
        for parent in &graph.commits[hash].parents {
            if let Some(degree) = indegree.get_mut(parent) {
                *degree += 1;
            }
        }
    }
    let mut queue: Vec<&String> = list.iter().filter(|hash| indegree[*hash] == 1).collect();
    if !date_order {
        queue.reverse();
    }
    let mut out = Vec::with_capacity(list.len());
    loop {
        let next = if date_order {
            // Newest first; among equal dates, the one queued first.
            let best = queue.iter().enumerate().fold(None, |best: Option<usize>, (i, hash)| match best {
                Some(b) if graph.date(queue[b]) >= graph.date(hash) => Some(b),
                _ => Some(i),
            });
            best.map(|i| queue.remove(i))
        } else {
            queue.pop()
        };
        let Some(commit) = next else { break };
        for parent in &graph.commits[commit].parents {
            if !in_list.contains(parent) {
                continue;
            }
            let Some(degree) = indegree.get_mut(parent) else { continue };
            if *degree == 0 {
                continue;
            }
            *degree -= 1;
            if *degree == 1 {
                queue.push(in_list.get(parent).copied().unwrap());
            }
        }
        indegree.insert(commit, 0);
        out.push(commit.clone());
    }
    out
}

/// Port of git's `name_commits`: gives every seen commit a name relative to a tip. Tips
/// get their own name, then first-parent ancestors get `tip~n`, then other parents get
/// names like `tip^2` (and their first-parent ancestors `tip^2~n`).
// Used by `show_branch_output`.
fn name_commits(graph: &Graph, list: &[String], tips: &[String], names: &[String]) -> HashMap<String, (String, usize)> {
    let mut named: HashMap<String, (String, usize)> = HashMap::new();
    for hash in list {
        if named.contains_key(hash) {
            continue;
        }
        if let Some(i) = tips.iter().position(|tip| tip == hash) {
            named.insert(hash.clone(), (names[i].clone(), 0));
        }
    }

    fn first_parent_chain(graph: &Graph, named: &mut HashMap<String, (String, usize)>, start: &str) -> usize {
        let mut count = 0;
        let mut current = start.to_string();
        loop {
            let Some((head_name, generation)) = named.get(&current).cloned() else { break };
            let Some(parent) = graph.commits.get(&current).and_then(|c| c.parents.first()).cloned() else { break };
            if named.contains_key(&parent) {
                break;
            }
            named.insert(parent.clone(), (head_name, generation + 1));
            count += 1;
            current = parent;
        }
        count
    }

    loop {
        let changed: usize = list.iter().map(|hash| first_parent_chain(graph, &mut named, hash)).sum();
        if changed == 0 {
            break;
        }
    }

    loop {
        let mut changed = 0;
        for hash in list {
            let Some((head_name, generation)) = named.get(hash).cloned() else { continue };
            for (nth, parent) in graph.commits[hash].parents.iter().enumerate() {
                if named.contains_key(parent) {
                    continue;
                }
                let mut name = match generation {
                    0 => head_name.clone(),
                    1 => format!("{}^", head_name),
                    n => format!("{}~{}", head_name, n),
                };
                if nth == 0 {
                    name.push('^');
                } else {
                    name.push_str(&format!("^{}", nth + 1));
                }
                named.insert(parent.clone(), (name, 0));
                changed += 1;
                first_parent_chain(graph, &mut named, parent);
            }
        }
        if changed == 0 {
            break;
        }
    }
    named
}

/// git's `show_one_commit`: `[name] subject`, where the name is the tip-relative name
/// (`topic~2`), the abbreviated hash with `--sha1-name`, or left out with `--no-name`; a
/// leading `[PATCH] ` is dropped from the subject.
// Used by `show_branch_output`.
fn one_commit_line(graph: &Graph, hash: &str, named: &HashMap<String, (String, usize)>, no_name: bool) -> String {
    let subject = graph.commits.get(hash).map(|c| split_commit_message(&c.message).0).unwrap_or_else(|| "(unavailable)".to_string());
    let subject = subject.strip_prefix("[PATCH] ").unwrap_or(&subject).to_string();
    if no_name {
        return format!("{}\n", subject);
    }
    match named.get(hash) {
        Some((head_name, 0)) => format!("[{}] {}\n", head_name, subject),
        Some((head_name, 1)) => format!("[{}^] {}\n", head_name, subject),
        Some((head_name, generation)) => format!("[{}~{}] {}\n", head_name, generation, subject),
        None => format!("[{}] {}\n", &hash[..7], subject),
    }
}

/// Port of the body of git's `cmd_show_branch`: returns the output and the exit status.
/// The default output has one header line per tip (`!` or `*` for the current branch,
/// indented by its column), a line of dashes, then the commits reachable from the tips down
/// to their common ancestor, each prefixed by one column per tip: `+` (or `*`) when the tip
/// contains the commit, `-` for a merge, blank otherwise. `--merge-base` prints the merge
/// bases of all tips (status 1 when there are none), `--independent` the tips not reachable
/// from any other tip, and `--list` just the header lines.
// Used by `show-branch`.
pub fn show_branch_output(opts: &ShowBranchOptions, names: &[String], head: Option<&str>) -> anyhow::Result<(String, i32)> {
    let mut graph = Graph { commits: HashMap::new(), flags: HashMap::new(), seen: Vec::new() };
    let mut queue: Vec<String> = Vec::new();
    let mut tips: Vec<String> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        if resolve_name(name).is_err() {
            anyhow::bail!("fatal: '{}' is not a valid ref.", name);
        }
        let Some(commit) = commit_of(name) else { anyhow::bail!("fatal: cannot find commit {}", name) };
        graph.load(&commit)?;
        graph.mark_seen(&commit);
        let flag = 1u32 << (i as u32 + REV_SHIFT);
        let entry = graph.flags.entry(commit.clone()).or_insert(0);
        *entry |= flag;
        if *entry == flag {
            let insert = commit.clone();
            graph.insert_by_date(&mut queue, &insert);
        }
        tips.push(commit);
    }
    let num_rev = tips.len();
    let rev_mask: Vec<u32> = tips.iter().map(|tip| graph.flag(tip)).collect();
    let head_hash = head.and_then(|_| commit_of("HEAD"));

    if opts.extra >= 0 {
        join_revs(&mut graph, &mut queue, num_rev, opts.extra)?;
    }
    // git prepends to its seen list, then sorts it by date (newest first, stable).
    let mut seen: Vec<String> = graph.seen.iter().rev().cloned().collect();
    seen.sort_by_key(|hash| std::cmp::Reverse(graph.date(hash)));

    let all_mask: u32 = (1u32 << (REV_SHIFT + num_rev as u32)) - 1;
    let all_revs = all_mask & !((1u32 << REV_SHIFT) - 1);
    let mut out = String::new();

    if opts.merge_base {
        let mut status = 1;
        for hash in &seen {
            let flags = graph.flag(hash) & all_mask;
            if flags & UNINTERESTING == 0 && flags & all_revs == all_revs {
                out.push_str(&format!("{}\n", hash));
                status = 0;
                *graph.flags.entry(hash.clone()).or_insert(0) |= UNINTERESTING;
            }
        }
        return Ok((out, status));
    }
    if opts.independent {
        for (i, tip) in tips.iter().enumerate() {
            if graph.flag(tip) == rev_mask[i] {
                out.push_str(&format!("{}\n", tip));
            }
            *graph.flags.entry(tip.clone()).or_insert(0) |= UNINTERESTING;
        }
        return Ok((out, 0));
    }

    let no_names = HashMap::new();
    let mut head_at: Option<usize> = None;
    if num_rev > 1 || opts.extra < 0 {
        for (i, name) in names.iter().enumerate() {
            let is_head = rev_is_head(head, name) && head_hash.as_deref() == Some(tips[i].as_str());
            if opts.extra < 0 {
                out.push_str(&format!("{} [{}] ", if is_head { '*' } else { ' ' }, name));
            } else {
                out.push_str(&format!("{}{} [{}] ", " ".repeat(i), if is_head { '*' } else { '!' }, name));
            }
            out.push_str(&one_commit_line(&graph, &tips[i], &no_names, true));
            if is_head {
                head_at = Some(i);
            }
        }
        if opts.extra >= 0 {
            out.push_str(&"-".repeat(num_rev));
            out.push('\n');
        }
    }
    if opts.extra < 0 {
        return Ok((out, 0));
    }

    let seen = sort_topologically(&graph, seen, opts.date_order);
    let named = if opts.sha1_name || opts.no_name { HashMap::new() } else { name_commits(&graph, &seen, &tips, names) };

    let mut extra = opts.extra;
    let mut shown_merge_point = false;
    for hash in &seen {
        let this_flag = graph.flag(hash);
        let is_merge_point = this_flag & all_revs == all_revs;
        shown_merge_point |= is_merge_point;

        if num_rev > 1 {
            let is_merge = graph.commits[hash].parents.len() > 1;
            if opts.topics && !is_merge_point && this_flag & (1u32 << REV_SHIFT) != 0 {
                continue;
            }
            // A merge that only one tip contains (and that isn't a tip) is left out unless --sparse.
            let containing = (0..num_rev).filter(|i| this_flag & (1u32 << (*i as u32 + REV_SHIFT)) != 0).count();
            if !opts.sparse && is_merge && !tips.contains(hash) && containing == 1 {
                continue;
            }
            for i in 0..num_rev {
                out.push(if this_flag & (1u32 << (i as u32 + REV_SHIFT)) == 0 {
                    ' '
                } else if is_merge {
                    '-'
                } else if Some(i) == head_at {
                    '*'
                } else {
                    '+'
                });
            }
            out.push(' ');
        }
        out.push_str(&one_commit_line(&graph, hash, &named, opts.no_name));

        if shown_merge_point {
            extra -= 1;
            if extra < 0 {
                break;
            }
        }
    }
    Ok((out, 0))
}
