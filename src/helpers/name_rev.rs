use std::collections::HashMap;
use anyhow::Result;

use crate::helpers::objects::read_object;
use crate::helpers::rev_list::list_all_refs;
use crate::helpers::wildmatch::wildmatch;
use crate::objects::{NameRevOptions, NameRevTip, RevName, RevNames};
use crate::refs::read_ref;

/// Usage text of `name-rev`, printed for `-h` and usage errors.
pub const NAME_REV_USAGE: &str = "usage: rgit name-rev [<options>] <commit>...\n   or: rgit name-rev [<options>] --all\n   or: rgit name-rev [<options>] --annotate-stdin\n\n    --[no-]name-only      print only ref-based names (no object names)\n    --[no-]tags           only use tags to name the commits\n    --[no-]refs <pattern> only use refs matching <pattern>\n    --[no-]exclude <pattern>\n                          ignore refs matching <pattern>\n\n    --[no-]all            list all commits reachable from all refs\n    --[no-]annotate-stdin annotate text from stdin\n    --[no-]undefined      allow to print `undefined` names (default)\n    --[no-]always         show abbreviated commit object as fallback\n\n";

/// Cost of following a second or later parent; first-parent steps cost 1.
const MERGE_TRAVERSAL_WEIGHT: u32 = 65535;
/// Commits more than a day older than the oldest requested commit are not named.
pub const CUTOFF_DATE_SLOP: i64 = 86400;

/// Parses the arguments of `name-rev`. Errors are the text to print before exiting 129.
// Used by `name-rev`.
pub fn parse_name_rev_args(args: &[String]) -> Result<NameRevOptions, String> {
    let mut opts = NameRevOptions::default();
    let mut options_done = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if options_done || !arg.starts_with('-') || arg == "-" {
            opts.revs.push(arg.to_string());
            continue;
        }
        if arg == "--" {
            options_done = true;
            continue;
        }
        if arg == "-h" {
            return Err(NAME_REV_USAGE.to_string());
        }
        let Some(body) = arg.strip_prefix("--") else {
            return Err(format!("error: unknown switch `{}'\n{}", arg.chars().nth(1).unwrap_or('-'), NAME_REV_USAGE));
        };
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (body, None),
        };
        let mut value = |name: &str| -> Result<String, String> {
            if let Some(value) = inline.clone() {
                return Ok(value);
            }
            let next = args.get(i).cloned().ok_or_else(|| format!("error: option `{}' requires a value\n", name))?;
            i += 1;
            Ok(next)
        };
        match name {
            "name-only" => opts.name_only = true,
            "no-name-only" => opts.name_only = false,
            "tags" => opts.tags = true,
            "no-tags" => opts.tags = false,
            "refs" => opts.refs.push(value("refs")?),
            "no-refs" => opts.refs.clear(),
            "exclude" => opts.exclude.push(value("exclude")?),
            "no-exclude" => opts.exclude.clear(),
            "all" => opts.all = true,
            "no-all" => opts.all = false,
            "annotate-stdin" | "stdin" => opts.annotate_stdin = true,
            "no-annotate-stdin" | "no-stdin" => opts.annotate_stdin = false,
            "undefined" => opts.no_undefined = false,
            "no-undefined" => opts.no_undefined = true,
            "always" => opts.always = true,
            "no-always" => opts.always = false,
            _ => return Err(format!("error: unknown option `{}'\n{}", body, NAME_REV_USAGE)),
        }
    }
    Ok(opts)
}

/// Parents and committer date of a commit; `None` when the object is missing or not a commit.
// Used by `build_rev_names` and `name-rev`.
pub fn commit_parents_and_date(hash: &str) -> Option<(Vec<String>, i64)> {
    let (object_type, content) = read_object(hash).ok()?;
    if object_type != "commit" {
        return None;
    }
    let text = String::from_utf8_lossy(&content);
    let mut parents = Vec::new();
    let mut date = 0;
    for line in text.lines().take_while(|l| !l.is_empty()) {
        if let Some(parent) = line.strip_prefix("parent ") {
            parents.push(parent.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("committer ") {
            date = signature_date(rest);
        }
    }
    Some((parents, date))
}

/// The unix time in an `author` / `committer` / `tagger` value (`Name <email> <time> <tz>`).
// Used by `commit_parents_and_date` and `peel_ref_tip`.
fn signature_date(value: &str) -> i64 {
    value.rsplit_once('>').and_then(|(_, rest)| rest.split_whitespace().next()).and_then(|t| t.parse().ok()).unwrap_or(0)
}

/// Follows annotated tags from `hash` to the object they finally name. Returns that object's
/// hash and type, whether a tag was peeled on the way, and the date of the last tag peeled.
// Used by `build_rev_names` and `name-rev`.
pub fn peel_ref_tip(hash: &str) -> Option<(String, String, bool, Option<i64>)> {
    let mut current = hash.to_string();
    let mut deref = false;
    let mut tag_date = None;
    for _ in 0..32 {
        let (object_type, content) = read_object(&current).ok()?;
        if object_type != "tag" {
            return Some((current, object_type, deref, tag_date));
        }
        let text = String::from_utf8_lossy(&content);
        let header: Vec<&str> = text.lines().take_while(|l| !l.is_empty()).collect();
        let target = header.iter().find_map(|l| l.strip_prefix("object "))?.trim().to_string();
        tag_date = Some(header.iter().find_map(|l| l.strip_prefix("tagger ")).map(signature_date).unwrap_or(0));
        deref = true;
        current = target;
    }
    None
}

/// Offset in `path` of the first trailing run of path components that `pattern` matches as a
/// whole (0 = the full ref name matches), or `None` when no such run matches.
// Used by `build_rev_names` for `--refs` and `--exclude`.
fn subpath_match(path: &str, pattern: &str) -> Option<usize> {
    let mut start = 0;
    loop {
        if wildmatch(pattern, &path[start..], 0) {
            return Some(start);
        }
        match path[start..].find('/') {
            Some(slash) => start += slash + 1,
            None => return None,
        }
    }
}

/// The shortest name that still means `refname` under git's lookup rules: `refs/tags/v1`
/// becomes `v1`, or `tags/v1` when something that is looked up earlier has that short name.
// Used by `ref_display_name`.
fn shorten_unambiguous_ref(refname: &str) -> String {
    const RULES: [(&str, &str); 6] = [("", ""), ("refs/", ""), ("refs/tags/", ""), ("refs/heads/", ""), ("refs/remotes/", ""), ("refs/remotes/", "/HEAD")];
    let exists = |name: String| crate::repo::path(format!(".git/{}", name)).is_file() || read_ref(&name).ok().flatten().is_some();
    for i in (1..RULES.len()).rev() {
        let (prefix, suffix) = RULES[i];
        let Some(short) = refname.strip_prefix(prefix).and_then(|rest| rest.strip_suffix(suffix)) else { continue };
        if short.is_empty() {
            continue;
        }
        let shadowed = RULES[..i].iter().any(|(p, s)| exists(format!("{}{}{}", p, short, s)));
        if !shadowed {
            return short.to_string();
        }
    }
    refname.to_string()
}

/// The name a ref goes by in `name-rev` output: branches lose `refs/heads/`, everything else
/// `refs/` (`tags/v1`, `remotes/origin/main`); with `shorten`, the shortest unambiguous name.
// Used by `build_rev_names`.
fn ref_display_name(refname: &str, shorten: bool) -> String {
    if shorten {
        shorten_unambiguous_ref(refname)
    } else if let Some(branch) = refname.strip_prefix("refs/heads/") {
        branch.to_string()
    } else {
        refname.strip_prefix("refs/").unwrap_or(refname).to_string()
    }
}

/// Whether a candidate name beats the one a commit already has: between two tags the nearer
/// one, a tag over any other ref, then the nearer ref, then the older tag/commit date.
// Used by `build_rev_names`.
fn is_better_name(current: &RevName, taggerdate: i64, generation: u32, distance: u32, from_tag: bool) -> bool {
    let effective = |distance: u32, generation: u32| distance as u64 + if generation > 0 { MERGE_TRAVERSAL_WEIGHT as u64 } else { 0 };
    let current_distance = effective(current.distance, current.generation);
    let new_distance = effective(distance, generation);
    if from_tag && current.from_tag {
        return current_distance > new_distance;
    }
    if current.from_tag != from_tag {
        return from_tag;
    }
    if current_distance != new_distance {
        return current_distance > new_distance;
    }
    current.taggerdate > taggerdate
}

/// Names every commit reachable from the refs the options allow, exactly like git: each ref
/// tip is named after its ref and the name spreads to the ancestors (`~N` along first
/// parents, `^N` into the other parents of a merge), a commit keeping the best name offered.
/// Tags are tried first, oldest first, so their names win. Commits older than `cutoff` are
/// left alone.
// Used by `name-rev`.
pub fn build_rev_names(opts: &NameRevOptions, cutoff: i64) -> RevNames {
    let mut names = RevNames::default();
    let mut tips: Vec<NameRevTip> = Vec::new();
    for (refname, hash) in list_all_refs() {
        if opts.tags && !refname.starts_with("refs/tags/") {
            continue;
        }
        if opts.exclude.iter().any(|pattern| subpath_match(&refname, pattern).is_some()) {
            continue;
        }
        let mut shorten = opts.tags && opts.name_only;
        if !opts.refs.is_empty() {
            let mut matched = false;
            for pattern in &opts.refs {
                match subpath_match(&refname, pattern) {
                    None => {}
                    Some(0) => matched = true,
                    // The pattern matched a shortened form, so a shortened name is wanted.
                    Some(_) => {
                        matched = true;
                        shorten = true;
                    }
                }
            }
            if !matched {
                continue;
            }
        }

        let display = ref_display_name(&refname, shorten);
        names.tips.entry(hash.clone()).or_insert_with(|| display.clone());
        let from_tag = refname.starts_with("refs/tags/");
        let mut tip = NameRevTip { name: display, commit: None, taggerdate: i64::MAX, from_tag: false, deref: false };
        if let Some((target, object_type, deref, tag_date)) = peel_ref_tip(&hash) {
            tip.deref = deref;
            if let Some(date) = tag_date {
                tip.taggerdate = date;
            }
            if object_type == "commit" {
                if tag_date.is_none() {
                    tip.taggerdate = commit_parents_and_date(&target).map(|(_, date)| date).unwrap_or(0);
                }
                tip.from_tag = from_tag;
                tip.commit = Some(target);
            }
        }
        tips.push(tip);
    }

    // Better names first, so that worse ones spread less: tags, then older dates.
    tips.sort_by(|a, b| b.from_tag.cmp(&a.from_tag).then(a.taggerdate.cmp(&b.taggerdate)));

    let mut cache: HashMap<String, Option<(Vec<String>, i64)>> = HashMap::new();
    let mut load = |hash: &str| -> Option<(Vec<String>, i64)> { cache.entry(hash.to_string()).or_insert_with(|| commit_parents_and_date(hash)).clone() };

    for tip in &tips {
        let Some(start) = &tip.commit else { continue };
        let Some((_, start_date)) = load(start) else { continue };
        if start_date < cutoff {
            continue;
        }
        if names.commits.get(start).is_some_and(|current| !is_better_name(current, tip.taggerdate, 0, 0, tip.from_tag)) {
            continue;
        }
        let tip_name = if tip.deref { format!("{}^0", tip.name) } else { tip.name.clone() };
        names.commits.insert(start.clone(), RevName { tip_name, taggerdate: tip.taggerdate, generation: 0, distance: 0, from_tag: tip.from_tag });

        let mut stack: Vec<String> = vec![start.clone()];
        while let Some(commit) = stack.pop() {
            let Some(name) = names.commits.get(&commit).cloned() else { continue };
            let Some((parents, _)) = load(&commit) else { continue };
            let mut to_queue: Vec<String> = Vec::new();
            for (index, parent) in parents.iter().enumerate() {
                let parent_number = index + 1;
                let Some((_, parent_date)) = load(parent) else { continue };
                if parent_date < cutoff {
                    continue;
                }
                let (generation, distance) = if parent_number > 1 {
                    (0, name.distance.saturating_add(MERGE_TRAVERSAL_WEIGHT))
                } else {
                    (name.generation + 1, name.distance.saturating_add(1))
                };
                if names.commits.get(parent).is_some_and(|current| !is_better_name(current, tip.taggerdate, generation, distance, tip.from_tag)) {
                    continue;
                }
                let tip_name = if parent_number > 1 {
                    let base = name.tip_name.strip_suffix("^0").unwrap_or(&name.tip_name);
                    if name.generation > 0 { format!("{}~{}^{}", base, name.generation, parent_number) } else { format!("{}^{}", base, parent_number) }
                } else {
                    name.tip_name.clone()
                };
                names.commits.insert(parent.clone(), RevName { tip_name, taggerdate: tip.taggerdate, generation, distance, from_tag: tip.from_tag });
                to_queue.push(parent.clone());
            }
            // The first parent must be handled first.
            stack.extend(to_queue.into_iter().rev());
        }
    }
    names
}

/// The printable form of a commit's name: `main`, `tags/v1^0`, `main~3`, `main~3^2~1`.
// Used by `object_name`.
fn rev_name_text(name: &RevName) -> String {
    if name.generation == 0 {
        name.tip_name.clone()
    } else {
        format!("{}~{}", name.tip_name.strip_suffix("^0").unwrap_or(&name.tip_name), name.generation)
    }
}

/// The name of an object: a commit's best ref-relative name, or for any other object the ref
/// that points straight at it. `None` when it has no name.
// Used by `name-rev`.
pub fn object_name(names: &RevNames, hash: &str, is_commit: bool) -> Option<String> {
    if is_commit { names.commits.get(hash).map(rev_name_text) } else { names.tips.get(hash).cloned() }
}

/// Annotates one line of text for `--annotate-stdin`: every full 40-digit lowercase hash
/// that has a name gets ` (name)` appended, or is replaced by the name with `name_only`.
// Used by `name-rev --annotate-stdin`.
pub fn annotate_line(names: &RevNames, line: &str, name_only: bool) -> String {
    let is_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut copied = 0;
    let mut run = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if !is_hex(b) {
            run = 0;
            continue;
        }
        run += 1;
        if run != 40 || bytes.get(i + 1).is_some_and(|next| is_hex(*next)) {
            continue;
        }
        run = 0;
        let hash = &line[i + 1 - 40..=i];
        let name = match names.commits.get(hash) {
            Some(name) => Some(rev_name_text(name)),
            None => names.tips.get(hash).cloned(),
        };
        let Some(name) = name else { continue };
        if name_only {
            out.push_str(&line[copied..i + 1 - 40]);
            out.push_str(&name);
        } else {
            out.push_str(&line[copied..=i]);
            out.push_str(&format!(" ({})", name));
        }
        copied = i + 1;
    }
    out.push_str(&line[copied..]);
    out
}
