use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use regex::{Regex, RegexBuilder};
use crate::helpers::bisect::collect_ancestors;
use crate::helpers::format_patch::{civil_from_days, parse_commit_object, resolve_commit_rev, split_commit_message, tz_offset_seconds};
use crate::helpers::ls_files::{matches_ls_files_pathspec, normalize_pathspecs};
use crate::helpers::merge::find_all_merge_bases;
use crate::helpers::flatten_tree;
use crate::objects::{CommitSignature, ParsedCommit, RevListFormat, RevListOptions, RevListRoots, RevWalkState, WalkedCommit};

const USAGE: &str = "usage: rgit rev-list [<options>] <commit>... [--] [<path>...]";

/// Parses `rev-list` arguments the way git does: options may appear anywhere before `--`,
/// `--not` flips the sense of the revisions that follow it, `-<n>` / `-n <n>` / `-n<n>` /
/// `--max-count=<n>` limit the count, and non-revision arguments that name existing files
/// start the path list. Errors starting with `usage:` mean exit 129, others 128.
// Used by `rev-list`.
pub fn parse_rev_list_args(args: &[String]) -> anyhow::Result<RevListOptions> {
    let opts = parse_revision_args(args)?;
    if opts.revs.is_empty() && !(opts.all || opts.branches || opts.tags || opts.remotes) {
        anyhow::bail!("{}", USAGE);
    }
    Ok(opts)
}

/// Like `parse_rev_list_args`, but an empty revision list is allowed (the caller picks a
/// default, as `log` does with `HEAD`).
// Used by `parse_rev_list_args` and `log`.
pub fn parse_revision_args(args: &[String]) -> anyhow::Result<RevListOptions> {
    let mut opts = RevListOptions::default();
    let mut negate = false;
    let mut i = 0;

    let value = |arg: &str, args: &[String], i: &mut usize, name: &str| -> anyhow::Result<String> {
        if let Some(v) = arg.strip_prefix(&format!("{}=", name)) {
            return Ok(v.to_string());
        }
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| anyhow::anyhow!("fatal: option '{}' requires a value", name.trim_start_matches('-')))
    };
    let number = |text: &str, name: &str| -> anyhow::Result<usize> {
        text.parse().map_err(|_| anyhow::anyhow!("fatal: '{}': not an integer", text)).and_then(|n: usize| {
            let _ = name;
            Ok(n)
        })
    };

    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            opts.paths.extend(args[i + 1..].iter().cloned());
            break;
        }
        if let Some(n) = arg.strip_prefix('-').filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
            opts.max_count = Some(number(n, "-<n>")?);
        } else if arg == "-n" || arg == "--max-count" || arg.starts_with("--max-count=") {
            let v = value(arg, args, &mut i, "--max-count")?;
            opts.max_count = Some(number(&v, "--max-count")?);
        } else if let Some(n) = arg.strip_prefix("-n").filter(|n| !n.is_empty()) {
            opts.max_count = Some(number(n, "-n")?);
        } else if arg == "--skip" || arg.starts_with("--skip=") {
            opts.skip = number(&value(arg, args, &mut i, "--skip")?, "--skip")?;
        } else if arg == "--min-parents" || arg.starts_with("--min-parents=") {
            opts.min_parents = Some(number(&value(arg, args, &mut i, "--min-parents")?, "--min-parents")?);
        } else if arg == "--max-parents" || arg.starts_with("--max-parents=") {
            opts.max_parents = Some(number(&value(arg, args, &mut i, "--max-parents")?, "--max-parents")?);
        } else if ["--since", "--after"].iter().any(|n| arg == *n || arg.starts_with(&format!("{}=", n))) {
            let name = if arg.starts_with("--since") { "--since" } else { "--after" };
            let v = value(arg, args, &mut i, name)?;
            opts.since = Some(parse_approx_date(&v).ok_or_else(|| anyhow::anyhow!("fatal: invalid date '{}'", v))?);
        } else if ["--until", "--before"].iter().any(|n| arg == *n || arg.starts_with(&format!("{}=", n))) {
            let name = if arg.starts_with("--until") { "--until" } else { "--before" };
            let v = value(arg, args, &mut i, name)?;
            opts.until = Some(parse_approx_date(&v).ok_or_else(|| anyhow::anyhow!("fatal: invalid date '{}'", v))?);
        } else if arg == "--author" || arg.starts_with("--author=") {
            opts.authors.push(value(arg, args, &mut i, "--author")?);
        } else if arg == "--committer" || arg.starts_with("--committer=") {
            opts.committers.push(value(arg, args, &mut i, "--committer")?);
        } else if arg == "--grep" || arg.starts_with("--grep=") {
            opts.greps.push(value(arg, args, &mut i, "--grep")?);
        } else if arg == "--format" || arg.starts_with("--format=") {
            let template = value(arg, args, &mut i, "--format")?;
            opts.format = Some(RevListFormat::Template { template, terminator: true });
        } else if arg == "--pretty" || arg.starts_with("--pretty=") {
            let v = if arg == "--pretty" { "medium".to_string() } else { value(arg, args, &mut i, "--pretty")? };
            opts.format = Some(if v == "oneline" {
                RevListFormat::Oneline
            } else if let Some(t) = v.strip_prefix("format:") {
                RevListFormat::Template { template: t.to_string(), terminator: false }
            } else if let Some(t) = v.strip_prefix("tformat:") {
                RevListFormat::Template { template: t.to_string(), terminator: true }
            } else if v.contains('%') {
                RevListFormat::Template { template: v, terminator: true }
            } else {
                anyhow::bail!("fatal: unsupported pretty format '{}' (use oneline, format:<fmt>, or tformat:<fmt>)", v);
            });
        } else if arg.starts_with("--") || (arg.starts_with('-') && arg.len() > 1) {
            match arg {
                "--not" => negate = !negate,
                "--all" | "--branches" | "--tags" | "--remotes" => {
                    match arg {
                        "--all" => opts.all = true,
                        "--branches" => opts.branches = true,
                        "--tags" => opts.tags = true,
                        _ => opts.remotes = true,
                    }
                    opts.revs.push(if negate { format!("^{}", arg) } else { arg.to_string() });
                }
                "--reverse" => opts.reverse = true,
                "--count" => opts.count = true,
                "--parents" => opts.parents = true,
                "--timestamp" => opts.timestamp = true,
                "--left-right" => opts.left_right = true,
                "--first-parent" => opts.first_parent = true,
                "--topo-order" => {
                    opts.topo_order = true;
                    opts.date_order = false;
                }
                "--date-order" => {
                    opts.date_order = true;
                    opts.topo_order = false;
                }
                "--abbrev-commit" => opts.abbrev_commit = true,
                "--no-abbrev-commit" => opts.abbrev_commit = false,
                "--oneline" => {
                    opts.format = Some(RevListFormat::Oneline);
                    opts.abbrev_commit = true;
                }
                "--merges" => opts.min_parents = Some(2),
                "--no-merges" => opts.max_parents = Some(1),
                "--no-min-parents" => opts.min_parents = None,
                "--no-max-parents" => opts.max_parents = None,
                "--all-match" => opts.all_match = true,
                "--invert-grep" => opts.invert_grep = true,
                "-i" | "--regexp-ignore-case" => opts.ignore_case = true,
                "-F" | "--fixed-strings" => opts.fixed_strings = true,
                "-E" | "--extended-regexp" => {}
                _ => anyhow::bail!("{}", USAGE),
            }
        } else if is_revision_arg(arg) {
            let rev = if negate {
                match arg.strip_prefix('^') {
                    Some(plain) => plain.to_string(),
                    None if arg.contains("..") => arg.to_string(),
                    None => format!("^{}", arg),
                }
            } else {
                arg.to_string()
            };
            opts.revs.push(rev);
        } else if Path::new(arg).exists() {
            opts.paths.extend(args[i..].iter().filter(|a| a.as_str() != "--").cloned());
            break;
        } else {
            anyhow::bail!(
                "fatal: ambiguous argument '{}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'rgit <command> [<revision>...] -- [<file>...]'",
                arg
            );
        }
        i += 1;
    }
    Ok(opts)
}

/// Runs a whole revision selection: the commit filters, the walk, the optional topological
/// sort, the parent-count/date/grep filters, `--skip`, and `--max-count`. Returns the selected
/// commits newest first (before `--reverse`) together with the walk state.
// Used by `rev-list` and `log`.
pub fn select_rev_list_commits(opts: &RevListOptions) -> anyhow::Result<(Vec<WalkedCommit>, RevWalkState)> {
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
    let selected = walked
        .into_iter()
        .filter(|w| !w.hidden && !state.uninteresting.contains(&w.commit.hash))
        .filter(|w| shown(&w.commit, w.parents.len()))
        .skip(opts.skip)
        .take(opts.max_count.unwrap_or(usize::MAX))
        .collect();
    Ok((selected, state))
}

fn is_revision_arg(arg: &str) -> bool {
    let check = |r: &str| r.is_empty() || resolve_commit_rev(r).is_ok();
    if let Some((a, b)) = arg.split_once("...") {
        return check(a) && check(b);
    }
    if let Some((a, b)) = arg.split_once("..") {
        return check(a) && check(b);
    }
    check(arg.strip_prefix('^').unwrap_or(arg)) && !arg.is_empty()
}

/// Parses the date forms `--since` / `--until` accept: Unix timestamps (`1700000000` or
/// `@1700000000`), `YYYY-MM-DD[ HH:MM[:SS]]` with optional `T` separator and `+HHMM`/`Z`
/// zone (UTC when absent), `now`, `yesterday`, and `<n> <unit>[s] ago` (also dotted, like
/// `2.weeks.ago`) for seconds through years.
// Used by `rev-list`.
pub fn parse_approx_date(text: &str) -> Option<i64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let t = text.trim().to_ascii_lowercase();
    if let Some(ts) = t.strip_prefix('@').unwrap_or(&t).parse::<i64>().ok().filter(|_| t.starts_with('@') || t.len() >= 9) {
        return Some(ts);
    }
    match t.as_str() {
        "now" => return Some(now),
        "yesterday" => return Some(now - 86_400),
        _ => {}
    }

    let words: Vec<&str> = t.split(|c: char| c == ' ' || c == '.').filter(|w| !w.is_empty()).collect();
    if words.len() == 3 && words[2] == "ago" {
        let n: i64 = words[0].parse().ok()?;
        let unit = words[1].trim_end_matches('s');
        let seconds = match unit {
            "second" => 1,
            "minute" => 60,
            "hour" => 3600,
            "day" => 86_400,
            "week" => 7 * 86_400,
            "month" => 30 * 86_400,
            "year" => 365 * 86_400,
            _ => return None,
        };
        return Some(now - n * seconds);
    }

    let re = Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(?:[ t](\d{1,2}):(\d{2})(?::(\d{2}))?)?\s*(z|[+-]\d{4})?$").ok()?;
    let caps = re.captures(&t)?;
    let num = |i: usize| caps.get(i).map(|m| m.as_str().parse::<i64>().unwrap_or(0)).unwrap_or(0);
    let (year, month, day) = (num(1), num(2), num(3));
    let local = days_from_civil(year, month, day) * 86_400 + num(4) * 3600 + num(5) * 60 + num(6);
    let tz = caps.get(7).map(|m| m.as_str()).filter(|z| *z != "z").unwrap_or("+0000");
    Some(local - tz_offset_seconds(tz))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Collects the commit hashes refs point at, as git adds them for `--all` and friends: every
/// matching ref under `.git/refs` and in `packed-refs` in name order (tags peeled to commits,
/// refs to other objects skipped), then `HEAD` for `--all`.
// Used by `rev-list`.
pub fn collect_ref_tips(all: bool, branches: bool, tags: bool, remotes: bool) -> anyhow::Result<Vec<String>> {
    let refs = list_all_refs();

    let mut tips = Vec::new();
    for (name, hash) in refs {
        let wanted = all
            || (branches && name.starts_with("refs/heads/"))
            || (tags && name.starts_with("refs/tags/"))
            || (remotes && name.starts_with("refs/remotes/"));
        if !wanted || name == "refs/stash" {
            continue;
        }
        if let Ok(commit) = crate::helpers::rev_parse::peel_to_type(hash, "commit") {
            tips.push(commit);
        }
    }
    if all {
        if let Ok(Some(head)) = crate::refs::resolve_head_commit() {
            tips.push(head);
        }
    }
    Ok(tips)
}

/// Lists every ref under `.git/refs` and in `packed-refs` (loose refs win) with the object
/// it points at, in name order. Symbolic refs (`refs/remotes/origin/HEAD` holding
/// `ref: refs/remotes/origin/main`) are listed with their target's object.
// Used by `collect_ref_tips`, `bundle create --all`, `log` decorations, and `tag`.
pub fn list_all_refs() -> BTreeMap<String, String> {
    let mut refs: BTreeMap<String, String> = BTreeMap::new();
    if let Ok(packed) = fs::read_to_string(crate::repo::path(".git/packed-refs")) {
        for line in packed.lines() {
            if let Some((hash, name)) = line.split_once(' ') {
                if hash.len() == 40 && !line.starts_with('#') {
                    refs.insert(name.to_string(), hash.to_string());
                }
            }
        }
    }
    let mut symbolic = Vec::new();
    collect_loose_refs(&crate::repo::path(".git/refs"), "refs", &mut refs, &mut symbolic);
    for (name, target) in symbolic {
        if let Some(hash) = refs.get(&target).cloned() {
            refs.insert(name, hash);
        }
    }
    refs
}

fn collect_loose_refs(dir: &Path, prefix: &str, out: &mut BTreeMap<String, String>, symbolic: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = format!("{}/{}", prefix, entry.file_name().to_string_lossy());
        let path = entry.path();
        if path.is_dir() {
            collect_loose_refs(&path, &name, out, symbolic);
        } else if let Ok(content) = fs::read_to_string(&path) {
            let hash = content.trim();
            if hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                out.insert(name, hash.to_string());
            } else if let Some(target) = hash.strip_prefix("ref: ") {
                symbolic.push((name, target.trim().to_string()));
            }
        }
    }
}

/// Resolves revision arguments, in order, into the pending list git builds: `X` is positive,
/// `^X` negative, `A..B` adds `A` negative then `B`, `A...B` adds the merge bases negative then
/// `A` (left) and `B`, and `--all`/`--branches`/`--tags`/`--remotes` (possibly after `--not`)
/// add every matching ref. Explicitly excluded commits are recorded as bottoms.
// Used by `rev-list`.
pub fn resolve_rev_list_tips(opts: &RevListOptions) -> anyhow::Result<RevListRoots> {
    let mut roots = RevListRoots::default();
    let resolve = |r: &str| resolve_commit_rev(if r.is_empty() { "HEAD" } else { r });
    let negative = |hash: String, roots: &mut RevListRoots| {
        roots.bottoms.insert(hash.clone());
        roots.pending.push((hash, true, false));
    };

    for rev in &opts.revs {
        let (neg, plain) = match rev.strip_prefix('^') {
            Some(p) => (true, p),
            None => (false, rev.as_str()),
        };
        if let Some(kind) = plain.strip_prefix("--") {
            let tips = collect_ref_tips(kind == "all", kind == "branches", kind == "tags", kind == "remotes")?;
            for tip in tips {
                if neg {
                    negative(tip, &mut roots);
                } else {
                    roots.pending.push((tip, false, false));
                }
            }
        } else if let Some((a, b)) = rev.split_once("...") {
            let (a, b) = (resolve(a)?, resolve(b)?);
            for base in find_all_merge_bases(&a, &b)? {
                negative(base, &mut roots);
            }
            roots.pending.push((a, false, true));
            roots.pending.push((b, false, false));
        } else if let Some((a, b)) = rev.split_once("..") {
            negative(resolve(a)?, &mut roots);
            roots.pending.push((resolve(b)?, false, false));
        } else if neg {
            negative(resolve(plain)?, &mut roots);
        } else {
            roots.pending.push((resolve(plain)?, false, false));
        }
    }
    Ok(roots)
}

/// Where `process_parents` puts newly seen parents: git's date-sorted commit list (limited
/// walk), the main date queue (unlimited walk), or the temporary queue used while rewriting
/// parents.
enum ParentSink<'a> {
    List(&'a mut Vec<String>),
    Queue(&'a mut BinaryHeap<(i64, Reverse<usize>, String)>, &'a mut usize),
    Temp(&'a mut Vec<(i64, usize, String)>, &'a mut usize),
}

fn commit_date(st: &RevWalkState, hash: &str) -> i64 {
    st.commits.get(hash).map(|c| c.committer.timestamp).unwrap_or(0)
}

/// Parses a commit into the walk state (git's `repo_parse_commit`). Only called where git
/// parses, because an unparsed commit has no known tree or parents: it can't be simplified and
/// marking UNINTERESTING doesn't spread through it, exactly as in git.
fn load_commit(st: &mut RevWalkState, hash: &str) -> anyhow::Result<()> {
    if !st.commits.contains_key(hash) {
        let commit = parse_commit_object(hash)?;
        st.parents.insert(hash.to_string(), commit.parents.clone());
        st.commits.insert(hash.to_string(), commit);
    }
    Ok(())
}

fn insert_by_date(list: &mut Vec<String>, st: &RevWalkState, hash: String) {
    let date = commit_date(st, &hash);
    let pos = list.iter().position(|h| commit_date(st, h) < date).unwrap_or(list.len());
    list.insert(pos, hash);
}

fn mark_parents_uninteresting(st: &mut RevWalkState, hash: &str) {
    let mut stack: Vec<String> = Vec::new();
    for p in st.parents.get(hash).cloned().unwrap_or_default() {
        mark_one_uninteresting(st, &p, &mut stack);
    }
    while let Some(p) = stack.pop() {
        mark_one_uninteresting(st, &p, &mut stack);
    }
}

fn mark_one_uninteresting(st: &mut RevWalkState, hash: &str, stack: &mut Vec<String>) {
    if !st.uninteresting.insert(hash.to_string()) {
        return;
    }
    if let Some(parents) = st.parents.get(hash) {
        stack.extend(parents.iter().cloned());
    }
}

fn temp_queue_put(queue: &mut Vec<(i64, usize, String)>, counter: &mut usize, date: i64, hash: String) {
    queue.push((date, *counter, hash));
    *counter += 1;
}

fn process_parents(st: &mut RevWalkState, hash: &str, opts: &RevListOptions, specs: &[String], trees: &mut HashMap<String, BTreeMap<String, ([u8; 20], u32)>>, sink: &mut ParentSink) -> anyhow::Result<()> {
    if !st.added.insert(hash.to_string()) {
        return Ok(());
    }
    if !st.commits.contains_key(hash) {
        return Ok(());
    }
    let push = |st: &mut RevWalkState, p: String, sink: &mut ParentSink| {
        if !st.seen.insert(p.clone()) {
            return;
        }
        let date = commit_date(st, &p);
        match sink {
            ParentSink::List(list) => insert_by_date(list, st, p),
            ParentSink::Queue(heap, seq) => {
                heap.push((date, Reverse(**seq), p));
                **seq += 1;
            }
            ParentSink::Temp(queue, counter) => temp_queue_put(queue, counter, date, p),
        }
    };

    if st.uninteresting.contains(hash) {
        for p in st.parents.get(hash).cloned().unwrap_or_default() {
            st.uninteresting.insert(p.clone());
            load_commit(st, &p)?;
            mark_parents_uninteresting(st, &p);
            push(st, p, sink);
        }
        return Ok(());
    }

    if !specs.is_empty() {
        simplify_commit(st, hash, opts.first_parent, specs, trees)?;
    }
    let parents = st.parents.get(hash).cloned().unwrap_or_default();
    let left = st.left.contains(hash);
    for p in parents.into_iter().take(if opts.first_parent { 1 } else { usize::MAX }) {
        load_commit(st, &p)?;
        if left {
            st.left.insert(p.clone());
        }
        push(st, p, sink);
    }
    Ok(())
}

fn simplify_commit(st: &mut RevWalkState, hash: &str, first_parent: bool, specs: &[String], trees: &mut HashMap<String, BTreeMap<String, ([u8; 20], u32)>>) -> anyhow::Result<()> {
    let tree = st.commits[hash].tree.clone();
    let own = filtered_tree(&tree, specs, trees)?;
    let parents = st.parents.get(hash).cloned().unwrap_or_default();
    if parents.is_empty() {
        if own.is_empty() {
            st.treesame.insert(hash.to_string());
        }
        return Ok(());
    }

    let (mut relevant_parents, mut relevant_change, mut irrelevant_change) = (0, false, false);
    for (n, parent) in parents.iter().enumerate() {
        let relevant = st.is_relevant(parent);
        if relevant {
            relevant_parents += 1;
        }
        if n == 1 && first_parent {
            break;
        }
        load_commit(st, parent)?;
        let parent_tree = st.commits[parent].tree.clone();
        let same = filtered_tree(&parent_tree, specs, trees)? == own;
        match (same, relevant) {
            (true, true) => {
                st.parents.insert(hash.to_string(), vec![parent.clone()]);
                st.treesame.insert(hash.to_string());
                return Ok(());
            }
            (true, false) => {}
            (false, true) => relevant_change = true,
            (false, false) => irrelevant_change = true,
        }
    }
    if !(if relevant_parents > 0 { relevant_change } else { irrelevant_change }) {
        st.treesame.insert(hash.to_string());
    }
    Ok(())
}

/// git's `one_relevant_parent`: the only parent (or the first with `--first-parent`), else the
/// sole relevant parent of a merge, else none.
fn one_relevant_parent(st: &RevWalkState, hash: &str, first_parent: bool) -> Option<String> {
    let parents = st.parents.get(hash)?;
    if parents.is_empty() {
        return None;
    }
    if first_parent || parents.len() == 1 {
        return Some(parents[0].clone());
    }
    let relevant: Vec<&String> = parents.iter().filter(|p| st.is_relevant(p)).collect();
    if relevant.len() == 1 { Some(relevant[0].clone()) } else { None }
}

/// Walks history by simulating git's revision machinery, flag for flag. With negative
/// revisions or `--topo-order`/`--date-order` the walk is "limited" (git's `limit_list`): a
/// date-sorted list is drained, UNINTERESTING spreads gradually from excluded commits, and
/// walking stops once only uninteresting commits remain (after git's slop of 5). Otherwise
/// commits are taken from a date queue as they are shown. With paths, each commit is
/// simplified when its parents are processed (`try_to_simplify_commit`), using relevance as
/// it stands at that moment, exactly like git; with `--parents`, showing a commit processes all
/// its parents and the hidden ancestors they are rewritten through, as git's `rewrite_one`
/// does, which can change the order and what gets walked (a parent the walk never parsed, like
/// a merge's second parent under `--first-parent`, stays as it is). `shown` is git's `get_commit_action` filter for the unlimited walk.
// Used by `rev-list`.
pub fn run_rev_list_walk(opts: &RevListOptions, roots: &RevListRoots, shown: &dyn Fn(&ParsedCommit, usize) -> bool) -> anyhow::Result<(Vec<WalkedCommit>, RevWalkState)> {
    let specs = normalize_pathspecs(&opts.paths);
    let mut trees = HashMap::new();
    let mut st = RevWalkState { bottoms: roots.bottoms.clone(), ..Default::default() };
    let limited = !roots.bottoms.is_empty() || opts.topo_order || opts.date_order;

    let mut initial: Vec<String> = Vec::new();
    for (hash, negative, left) in &roots.pending {
        load_commit(&mut st, hash)?;
        if *negative {
            st.uninteresting.insert(hash.clone());
            mark_parents_uninteresting(&mut st, hash);
        }
        if *left {
            st.left.insert(hash.clone());
        }
        if st.seen.insert(hash.clone()) {
            initial.push(hash.clone());
        }
    }
    let mut sorted_initial: Vec<(usize, String)> = initial.into_iter().enumerate().collect();
    sorted_initial.sort_by(|a, b| commit_date(&st, &b.1).cmp(&commit_date(&st, &a.1)).then(a.0.cmp(&b.0)));
    let initial: Vec<String> = sorted_initial.into_iter().map(|(_, h)| h).collect();

    let mut order: Vec<String> = Vec::new();
    if limited {
        const SLOP: i32 = 5;
        let mut list = initial;
        let mut slop = SLOP;
        let mut last_date = i64::MAX;
        while !list.is_empty() {
            let hash = list.remove(0);
            let date = commit_date(&st, &hash);
            if opts.since.is_some_and(|since| date < since) {
                st.uninteresting.insert(hash.clone());
            }
            process_parents(&mut st, &hash, opts, &specs, &mut trees, &mut ParentSink::List(&mut list))?;
            if st.uninteresting.contains(&hash) {
                mark_parents_uninteresting(&mut st, &hash);
                slop = match list.first() {
                    None => 0,
                    Some(top) if last_date <= commit_date(&st, top) => SLOP,
                    Some(_) if list.iter().any(|h| !st.uninteresting.contains(h)) => SLOP,
                    Some(_) => slop - 1,
                };
                if slop > 0 {
                    continue;
                }
                break;
            }
            if opts.until.is_some_and(|until| date >= until) {
                continue;
            }
            last_date = date;
            order.push(hash);
        }
    } else {
        let mut heap = BinaryHeap::new();
        let mut sequence = 0usize;
        for hash in initial {
            heap.push((commit_date(&st, &hash), Reverse(sequence), hash));
            sequence += 1;
        }
        while let Some((date, _, hash)) = heap.pop() {
            if opts.since.is_some_and(|since| date < since) {
                continue;
            }
            process_parents(&mut st, &hash, opts, &specs, &mut trees, &mut ParentSink::Queue(&mut heap, &mut sequence))?;
            let visible = !st.treesame.contains(&hash) && shown(&st.commits[&hash], st.parents[&hash].len());
            if visible && !specs.is_empty() && opts.parents {
                let mut temp = Vec::new();
                let mut counter = 0usize;
                let parents = st.parents[&hash].clone();
                for parent in parents {
                    let mut current = parent;
                    loop {
                        process_parents(&mut st, &current, opts, &specs, &mut trees, &mut ParentSink::Temp(&mut temp, &mut counter))?;
                        if st.uninteresting.contains(&current) || !st.treesame.contains(&current) {
                            break;
                        }
                        match one_relevant_parent(&st, &current, opts.first_parent) {
                            Some(next) => current = next,
                            None => break,
                        }
                    }
                }
                temp.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                for (date, _, h) in temp {
                    heap.push((date, Reverse(sequence), h));
                    sequence += 1;
                }
            }
            order.push(hash);
        }
    }

    let walked = order
        .into_iter()
        .map(|hash| WalkedCommit {
            commit: st.commits[&hash].clone(),
            parents: st.parents[&hash].clone(),
            hidden: st.treesame.contains(&hash),
            left: st.left.contains(&hash),
        })
        .collect();
    Ok((walked, st))
}

fn filtered_tree(tree: &str, specs: &[String], cache: &mut HashMap<String, BTreeMap<String, ([u8; 20], u32)>>) -> anyhow::Result<BTreeMap<String, ([u8; 20], u32)>> {
    if let Some(t) = cache.get(tree) {
        return Ok(t.clone());
    }
    let mut full = BTreeMap::new();
    flatten_tree(tree, "", &mut full)?;
    let filtered: BTreeMap<_, _> = full.into_iter().filter(|(p, _)| matches_ls_files_pathspec(p, specs)).collect();
    cache.insert(tree.to_string(), filtered.clone());
    Ok(filtered)
}

/// Port of git's `sort_in_topological_order` (over each commit's current parent list, which
/// simplification may have reduced): counts in-list children per commit, seeds the
/// queue with tips (in walk order), and emits a commit only after all its children. With
/// `by_date` (`--date-order`) the queue is ordered by committer date; otherwise
/// (`--topo-order`) it is a stack, which keeps each line of history together.
// Used by `rev-list`.
pub fn sort_rev_list_topologically(walked: Vec<WalkedCommit>, by_date: bool) -> Vec<WalkedCommit> {
    let index: HashMap<String, usize> = walked.iter().enumerate().map(|(i, w)| (w.commit.hash.clone(), i)).collect();
    let sort_parents = |w: &WalkedCommit| -> Vec<String> { w.parents.clone() };
    let mut indegree = vec![1usize; walked.len()];
    for w in &walked {
        for p in &sort_parents(w) {
            if let Some(&pi) = index.get(p) {
                indegree[pi] += 1;
            }
        }
    }

    let mut stack: Vec<usize> = Vec::new();
    let mut heap = BinaryHeap::new();
    let mut sequence = 0usize;
    let mut put = |i: usize, stack: &mut Vec<usize>, heap: &mut BinaryHeap<(i64, Reverse<usize>, usize)>| {
        if by_date {
            heap.push((walked[i].commit.committer.timestamp, Reverse(sequence), i));
            sequence += 1;
        } else {
            stack.push(i);
        }
    };
    for i in 0..walked.len() {
        if indegree[i] == 1 {
            put(i, &mut stack, &mut heap);
        }
    }
    if !by_date {
        stack.reverse();
    }

    let mut order = Vec::with_capacity(walked.len());
    loop {
        let next = if by_date { heap.pop().map(|(_, _, i)| i) } else { stack.pop() };
        let Some(i) = next else { break };
        for p in &sort_parents(&walked[i]) {
            if let Some(&pi) = index.get(p) {
                if indegree[pi] == 0 {
                    continue;
                }
                indegree[pi] -= 1;
                if indegree[pi] == 1 {
                    put(pi, &mut stack, &mut heap);
                }
            }
        }
        order.push(i);
    }

    let mut slots: Vec<Option<WalkedCommit>> = walked.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

/// Rewrites a shown commit's parents for path-limited history (git's `rewrite_parents`, using
/// the walk's final state): each parent hidden by simplification is replaced by following
/// `one_relevant_parent` until a shown or uninteresting commit (kept), a hidden commit with no
/// single relevant parent (kept), or a hidden root (dropped). Every parent is rewritten, even
/// with `--first-parent` (which only makes hidden commits follow their first parent).
/// Duplicates are removed.
// Used by `rev-list --parents` with paths.
pub fn rewrite_rev_list_parents(parents: &[String], st: &RevWalkState, first_parent: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for parent in parents {
        let mut current = parent.clone();
        let rewritten = loop {
            if st.uninteresting.contains(&current) || !st.treesame.contains(&current) {
                break Some(current);
            }
            if st.parents.get(&current).is_none_or(|p| p.is_empty()) {
                break None;
            }
            match one_relevant_parent(st, &current, first_parent) {
                Some(next) => current = next,
                None => break Some(current),
            }
        };
        if let Some(p) = rewritten {
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// Builds the `--author` / `--committer` / `--grep` filter: returns a predicate that keeps a
/// commit when every identity filter matches (any of its patterns) and the message filter
/// matches any pattern (all with `--all-match`), inverted with `--invert-grep`.
// Used by `rev-list`.
pub fn build_commit_filter(opts: &RevListOptions) -> anyhow::Result<impl Fn(&ParsedCommit) -> bool> {
    let compile = |patterns: &[String]| -> anyhow::Result<Vec<Regex>> {
        patterns
            .iter()
            .map(|p| {
                let p = if opts.fixed_strings { regex::escape(p) } else { p.clone() };
                RegexBuilder::new(&p)
                    .case_insensitive(opts.ignore_case)
                    .build()
                    .map_err(|e| anyhow::anyhow!("fatal: invalid regex '{}': {}", p, e))
            })
            .collect()
    };
    let authors = compile(&opts.authors)?;
    let committers = compile(&opts.committers)?;
    let greps = compile(&opts.greps)?;
    let (all_match, invert) = (opts.all_match, opts.invert_grep);

    Ok(move |c: &ParsedCommit| {
        let ident = |s: &CommitSignature| format!("{} <{}>", s.name, s.email);
        if !authors.is_empty() && !authors.iter().any(|r| r.is_match(&ident(&c.author))) {
            return false;
        }
        if !committers.is_empty() && !committers.iter().any(|r| r.is_match(&ident(&c.committer))) {
            return false;
        }
        if greps.is_empty() {
            return true;
        }
        let matched = if all_match {
            greps.iter().all(|r| r.is_match(&c.message))
        } else {
            greps.iter().any(|r| r.is_match(&c.message))
        };
        matched != invert
    })
}

/// Shortens a hash to the shortest unique prefix of at least 7 characters among loose objects.
// Used by `rev-list --abbrev-commit` / `--oneline`.
pub fn abbreviate_hash(hash: &str) -> String {
    let others: Vec<String> = fs::read_dir(crate::repo::path(format!(".git/objects/{}", &hash[..2])))
        .map(|entries| entries.flatten().map(|e| format!("{}{}", &hash[..2], e.file_name().to_string_lossy())).filter(|h| h != hash).collect())
        .unwrap_or_default();
    let mut len = 7;
    while len < hash.len() && others.iter().any(|o| o.starts_with(&hash[..len])) {
        len += 1;
    }
    hash[..len].to_string()
}

/// Expands a `--format` template for one commit. Supports `%H %h %T %t %P %p`, author
/// `%an %ae %ad %at %ai %aI`, committer `%cn %ce %cd %ct %ci %cI`, `%s %b %B`, `%n`, `%%`, and
/// `%x<hex>`; unknown placeholders are printed as is, like git.
// Used by `rev-list --format`.
pub fn expand_rev_list_format(template: &str, commit: &ParsedCommit, parents: &[String]) -> String {
    let (subject, body) = split_commit_message(&commit.message);
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '%' || i + 1 >= chars.len() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let two: String = chars[i + 1..(i + 3).min(chars.len())].iter().collect();
        let (text, used) = match chars[i + 1] {
            'H' => (commit.hash.clone(), 1),
            'h' => (abbreviate_hash(&commit.hash), 1),
            'T' => (commit.tree.clone(), 1),
            't' => (abbreviate_hash(&commit.tree), 1),
            'P' => (parents.join(" "), 1),
            'p' => (parents.iter().map(|p| abbreviate_hash(p)).collect::<Vec<_>>().join(" "), 1),
            's' => (subject.clone(), 1),
            'b' => (if body.is_empty() { String::new() } else { format!("{}\n", body) }, 1),
            'B' => (commit.message.clone(), 1),
            'n' => ("\n".to_string(), 1),
            '%' => ("%".to_string(), 1),
            'x' if two.len() == 2 => match u8::from_str_radix(&chars[i + 2..(i + 4).min(chars.len())].iter().collect::<String>(), 16) {
                Ok(b) => ((b as char).to_string(), 3),
                Err(_) => ("%x".to_string(), 1),
            },
            'a' | 'c' if two.len() == 2 => {
                let sig = if chars[i + 1] == 'a' { &commit.author } else { &commit.committer };
                match chars[i + 2] {
                    'n' => (sig.name.clone(), 2),
                    'e' => (sig.email.clone(), 2),
                    't' => (sig.timestamp.to_string(), 2),
                    'd' => (format_default_date(sig), 2),
                    'i' => (format_iso_date(sig, false), 2),
                    'I' => (format_iso_date(sig, true), 2),
                    _ => (format!("%{}", two), 2),
                }
            }
            other => (format!("%{}", other), 1),
        };
        out.push_str(&text);
        i += 1 + used;
    }
    out
}

fn local_parts(sig: &CommitSignature) -> (i64, i64, i64, i64, i64, i64, i64) {
    let local = sig.timestamp + tz_offset_seconds(&sig.tz);
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, secs / 3600, (secs / 60) % 60, secs % 60, days.rem_euclid(7))
}

fn format_default_date(sig: &CommitSignature) -> String {
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (y, m, d, h, mi, s, wd) = local_parts(sig);
    format!("{} {} {} {:02}:{:02}:{:02} {} {}", WEEKDAYS[wd as usize], MONTHS[(m - 1) as usize], d, h, mi, s, y, sig.tz)
}

fn format_iso_date(sig: &CommitSignature, strict: bool) -> String {
    let (y, m, d, h, mi, s, _) = local_parts(sig);
    if strict {
        let tz = if sig.tz.len() == 5 { format!("{}:{}", &sig.tz[..3], &sig.tz[3..]) } else { sig.tz.clone() };
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}", y, m, d, h, mi, s, tz)
    } else {
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}", y, m, d, h, mi, s, sig.tz)
    }
}
