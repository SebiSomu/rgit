use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;

use anyhow::{bail, Result};

use crate::helpers::diff_plumbing::quote_c_path;
use crate::helpers::objects::{flatten_tree, read_object};
use crate::helpers::rev_list::{list_all_refs, parse_rev_list_args, select_rev_list_commits};
use crate::helpers::rev_parse::resolve_rev;
use crate::objects::{DiffChange, DiffSide, FastExportOptions};
use crate::refs::read_ref;

/// Usage text of `fast-export`.
pub const FAST_EXPORT_USAGE: &str = "usage: rgit fast-export [<rev-list-opts>]\n\n    --signed-tags <when>  select handling of signed tags\n    --tag-of-filtered-object <when>\n                          select handling of tags that tag filtered objects\n    --export-marks <file> dump marks to this file\n    --import-marks <file> import marks from this file\n    --import-marks-if-exists <file>\n                          import marks from this file if it exists\n    --use-done-feature    use the done feature to terminate the stream\n    --no-data             skip output of blob data\n    --full-tree           output full tree for each commit\n    --mark-tags           label tags with mark ids\n    --reference-excluded-parents\n                          reference parents which are not in fast-export stream by object id\n    --show-original-ids   show original object ids of blobs/commits\n\n";

/// Parses the arguments of `fast-export`. Everything that is not one of its own options is a
/// revision argument (`<rev>`, `^<rev>`, `A..B`, `--all`, `--branches`, ...). `Err` is
/// `(text, exit status)`.
// Used by `fast-export`.
pub fn parse_fast_export_args(args: &[String]) -> Result<FastExportOptions, (String, i32)> {
    let mut opts = FastExportOptions { signed_tags: "abort".to_string(), tag_of_filtered: "abort".to_string(), ..Default::default() };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (arg, None),
        };
        let value = |i: &mut usize| -> Result<String, (String, i32)> {
            if let Some(value) = inline.clone() {
                return Ok(value);
            }
            *i += 1;
            args.get(*i - 1).cloned().ok_or_else(|| (format!("error: option `{}' requires a value\n", &name[2..]), 129))
        };
        match name {
            "-h" => return Err((FAST_EXPORT_USAGE.to_string(), 129)),
            "--signed-tags" => {
                opts.signed_tags = value(&mut i)?;
                if !matches!(opts.signed_tags.as_str(), "abort" | "verbatim" | "ignore" | "warn" | "warn-strip" | "strip") {
                    return Err((format!("error: Unknown signed-tags mode: {}\n", opts.signed_tags), 129));
                }
            }
            "--tag-of-filtered-object" => {
                opts.tag_of_filtered = value(&mut i)?;
                if !matches!(opts.tag_of_filtered.as_str(), "abort" | "drop") {
                    return Err((format!("fatal: --tag-of-filtered-object={} is not supported by rgit (use abort or drop)\n", opts.tag_of_filtered), 128));
                }
            }
            "--export-marks" => opts.export_marks = Some(value(&mut i)?),
            "--import-marks" => opts.import_marks = Some((value(&mut i)?, false)),
            "--import-marks-if-exists" => opts.import_marks = Some((value(&mut i)?, true)),
            "--no-data" => opts.no_data = true,
            "--data" => opts.no_data = false,
            "--full-tree" => opts.full_tree = true,
            "--use-done-feature" => opts.use_done = true,
            "--mark-tags" => opts.mark_tags = true,
            "--reference-excluded-parents" => opts.reference_excluded = true,
            "--show-original-ids" => opts.show_original_ids = true,
            "--progress" => {
                value(&mut i)?;
            }
            "--fake-missing-tagger" | "--no-progress" => {}
            _ if crate::helpers::rename::parse_rename_option(arg, &mut opts.renames) => {}
            "--anonymize" | "--anonymize-map" | "--refspec" | "--reencode" => {
                return Err((format!("fatal: {} is not supported by rgit fast-export\n", name), 128));
            }
            _ => opts.revs.push(arg.to_string()),
        }
    }
    Ok(opts)
}

/// The header lines and message of a raw commit or tag object.
// Used by `run_fast_export`.
fn split_object(content: &[u8]) -> (Vec<String>, Vec<u8>) {
    let split = content.windows(2).position(|w| w == b"\n\n");
    let (header, message) = match split {
        Some(at) => (&content[..at], content[at + 2..].to_vec()),
        None => (content, Vec::new()),
    };
    (String::from_utf8_lossy(header).lines().map(str::to_string).collect(), message)
}

/// A path as `fast-export` prints it: C-quoted when it has special characters, in plain
/// double quotes when it only has spaces.
// Used by `run_fast_export`.
fn export_path(path: &str) -> String {
    let quoted = quote_c_path(path);
    if quoted == path && path.contains(' ') { format!("\"{}\"", path) } else { quoted }
}

/// The full ref name a revision argument stands for, with git's lookup order, or `None`
/// when it is not a ref.
// Used by `run_fast_export`.
fn full_ref_name(name: &str) -> Option<String> {
    let mut candidates = Vec::new();
    if name.starts_with("refs/") {
        candidates.push(name.to_string());
    }
    for prefix in ["refs/", "refs/tags/", "refs/heads/", "refs/remotes/"] {
        candidates.push(format!("{}{}", prefix, name));
    }
    candidates.into_iter().find(|candidate| !crate::repo::path(format!(".git/{}", candidate)).is_dir() && read_ref(candidate).ok().flatten().is_some())
}

/// Writes a `git fast-import` stream for the selected history to `out`: every commit in an
/// order that puts parents first, each preceded by the blobs it introduces, with its file
/// changes against its first parent (`M <mode> :<mark> <path>` / `D <path>`); then a `reset`
/// for every other ref that points at an exported commit, and a `tag` for every annotated
/// tag. Commits are filed under the ref whose history reaches them first, as git does.
// Used by `fast-export`.
pub fn run_fast_export(opts: &FastExportOptions, out: &mut impl Write) -> Result<()> {
    let mut marks: HashMap<String, u32> = HashMap::new();
    let mut last_mark = 0u32;
    if let Some((file, if_exists)) = &opts.import_marks {
        match std::fs::read_to_string(file) {
            Ok(text) => {
                for line in text.lines() {
                    let Some((mark, hash)) = line.strip_prefix(':').and_then(|rest| rest.split_once(' ')) else { bail!("fatal: corrupt mark line: {}", line) };
                    let mark: u32 = mark.parse().map_err(|_| anyhow::anyhow!("fatal: corrupt mark line: {}", line))?;
                    marks.insert(hash.trim().to_string(), mark);
                    last_mark = last_mark.max(mark);
                }
            }
            Err(_) if *if_exists => {}
            Err(e) => bail!("fatal: cannot read '{}': {}", file, e),
        }
    }

    // Commits named in imported marks were exported before: they are not written again.
    let imported: HashSet<String> = marks.keys().cloned().collect();

    // The positive revision arguments, in order, with the ref (if any) they name.
    let mut tips: Vec<(String, String)> = Vec::new(); // (source name, object hash)
    let mut negate = false;
    for arg in &opts.revs {
        match arg.as_str() {
            "--not" => negate = !negate,
            "--all" | "--branches" | "--tags" | "--remotes" if !negate => {
                for (name, hash) in list_all_refs() {
                    let wanted = match arg.as_str() {
                        "--branches" => name.starts_with("refs/heads/"),
                        "--tags" => name.starts_with("refs/tags/"),
                        "--remotes" => name.starts_with("refs/remotes/"),
                        _ => true,
                    };
                    if wanted {
                        tips.push((name, hash));
                    }
                }
            }
            other if other.starts_with('-') => {}
            other => {
                let positive = match other.split_once("..") {
                    Some((_, right)) if !other.contains("...") => Some(if right.is_empty() { "HEAD" } else { right }),
                    Some(_) => None,
                    None if other.starts_with('^') => None,
                    None => Some(other),
                };
                if let Some(name) = positive.filter(|_| !negate) {
                    let hash = resolve_rev(name).map_err(|_| anyhow::anyhow!("fatal: ambiguous argument '{}': unknown revision or path not in the working tree.", name))?;
                    tips.push((full_ref_name(name).unwrap_or_else(|| name.to_string()), hash));
                }
            }
        }
    }

    // Which ref each tip commit is filed under; later refs on the same commit, and annotated
    // tags, are written at the end.
    let mut source: HashMap<String, String> = HashMap::new();
    let mut extra_refs: Vec<(String, String)> = Vec::new();
    let mut tag_refs: Vec<(String, String)> = Vec::new();
    let mut tip_commits: Vec<String> = Vec::new();
    for (name, hash) in &tips {
        let mut target = hash.clone();
        let mut is_tag = false;
        let mut kind = String::new();
        for _ in 0..32 {
            let Ok((object_kind, content)) = read_object(&target) else { break };
            kind = object_kind;
            if kind != "tag" {
                break;
            }
            is_tag = true;
            let (header, _) = split_object(&content);
            match header.iter().find_map(|l| l.strip_prefix("object ")) {
                Some(next) => target = next.trim().to_string(),
                None => break,
            }
        }
        if is_tag && name.starts_with("refs/") {
            tag_refs.push((name.clone(), hash.clone()));
        }
        if kind != "commit" {
            if kind != "blob" {
                eprintln!("warning: {}: Unexpected object of type {}, skipping.", name, kind);
            }
            continue;
        }
        if source.contains_key(&target) || imported.contains(&target) {
            // An annotated tag sets its ref through its own `tag` command.
            if name.starts_with("refs/") && !is_tag {
                extra_refs.push((name.clone(), target.clone()));
            }
        } else {
            source.insert(target.clone(), name.clone());
            tip_commits.push(target);
        }
    }

    // The commits to export, in topological order (newest first).
    let mut rev_args = vec!["--topo-order".to_string()];
    rev_args.extend(opts.revs.iter().cloned());
    for hash in &imported {
        if matches!(read_object(hash), Ok((ref kind, _)) if kind == "commit") {
            rev_args.push(format!("^{}", hash));
        }
    }
    let rev_opts = parse_rev_list_args(&rev_args)?;
    let (walked, _) = select_rev_list_commits(&rev_opts)?;
    let included: HashSet<&str> = walked.iter().map(|w| w.commit.hash.as_str()).collect();
    let by_hash: HashMap<&str, &crate::objects::ParsedCommit> = walked.iter().map(|w| (w.commit.hash.as_str(), &w.commit)).collect();

    // Spread the ref names from the tips to their ancestors the way git's walk does: commits
    // are taken newest first, and a parent inherits the name of the first child that reaches it.
    let date_of = |hash: &str| by_hash.get(hash).map(|c| c.committer.timestamp).unwrap_or(0);
    // Kept sorted newest first; a commit goes after the ones with the same date.
    let insert_by_date = |queue: &mut Vec<String>, hash: &str| {
        let date = date_of(hash);
        let at = queue.iter().position(|other| date_of(other) < date).unwrap_or(queue.len());
        queue.insert(at, hash.to_string());
    };
    let mut queue: Vec<String> = Vec::new();
    let mut queued: HashSet<String> = HashSet::new();
    for tip in &tip_commits {
        if by_hash.contains_key(tip.as_str()) && queued.insert(tip.clone()) {
            insert_by_date(&mut queue, tip);
        }
    }
    while !queue.is_empty() {
        let hash = queue.remove(0);
        let name = source.get(&hash).cloned();
        for parent in &by_hash[hash.as_str()].parents {
            if !by_hash.contains_key(parent.as_str()) {
                continue;
            }
            if let Some(name) = &name {
                source.entry(parent.clone()).or_insert_with(|| name.clone());
            }
            if queued.insert(parent.clone()) {
                insert_by_date(&mut queue, parent);
            }
        }
    }

    if opts.use_done {
        writeln!(out, "feature done")?;
    }

    // git walks the topological order backwards (oldest first) and handles a commit once all
    // its parents are shown, keeping any others on a stack.
    let mut exported_blobs: HashSet<String> = HashSet::new();
    let mut shown: HashSet<&str> = HashSet::new();
    let mut waiting: Vec<&str> = Vec::new();
    let has_unshown_parent = |hash: &str, shown: &HashSet<&str>| by_hash[hash].parents.iter().any(|p| included.contains(p.as_str()) && !shown.contains(p.as_str()));
    let mut order: Vec<&str> = Vec::new();
    for item in walked.iter().rev() {
        let hash = item.commit.hash.as_str();
        if has_unshown_parent(hash, &shown) {
            waiting.push(hash);
            continue;
        }
        shown.insert(hash);
        order.push(hash);
        while let Some(&next) = waiting.last() {
            if has_unshown_parent(next, &shown) {
                break;
            }
            waiting.pop();
            shown.insert(next);
            order.push(next);
        }
    }

    for hash in order {
        let (_, content) = read_object(hash)?;
        let (header, message) = split_object(&content);
        let field = |name: &str| header.iter().find_map(|l| l.strip_prefix(name)).map(str::to_string);
        let parents: Vec<String> = header.iter().filter_map(|l| l.strip_prefix("parent ")).map(str::to_string).collect();
        let tree = field("tree ").unwrap_or_default();
        let mut new_files: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
        flatten_tree(&tree, "", &mut new_files)?;
        let mut old_files: BTreeMap<String, ([u8; 20], u32)> = BTreeMap::new();
        let diff_base = parents.first().filter(|p| (marks.contains_key(*p) || opts.reference_excluded) && !opts.full_tree);
        if let Some(parent) = diff_base {
            let (_, parent_content) = read_object(parent)?;
            let parent_tree = split_object(&parent_content).0.iter().find_map(|l| l.strip_prefix("tree ")).unwrap_or("").to_string();
            flatten_tree(&parent_tree, "", &mut old_files)?;
        }

        // Changed paths in tree order (with -M / -C, renames and copies found); blobs first.
        let side = |e: &([u8; 20], u32)| DiffSide { mode: e.1, hash: e.0, worktree: false };
        let paths: std::collections::BTreeSet<&String> = old_files.keys().chain(new_files.keys()).collect();
        let mut diff: Vec<DiffChange> = Vec::new();
        for path in paths {
            let (old, new) = (old_files.get(path), new_files.get(path));
            if old != new {
                diff.push(DiffChange { path: path.clone(), old: old.map(side), new: new.map(side), renamed_from: None });
            }
        }
        let unmodified = if opts.renames.find_copies_harder { crate::helpers::diff_plumbing::unmodified_files(&old_files, &diff) } else { Vec::new() };
        diff = crate::helpers::rename::detect_renames(diff, &opts.renames, &unmodified);
        let changes: Vec<(String, Option<([u8; 20], u32)>)> = diff.iter().map(|c| (c.path.clone(), c.new.as_ref().map(|s| (s.hash, s.mode)))).collect();
        if !opts.no_data {
            for (_, new) in &changes {
                let Some((blob, mode)) = new else { continue };
                let name = hex::encode(blob);
                if *mode == 0o160000 || marks.contains_key(&name) {
                    continue;
                }
                let (_, data) = read_object(&name)?;
                last_mark += 1;
                marks.insert(name.clone(), last_mark);
                exported_blobs.insert(name.clone());
                writeln!(out, "blob\nmark :{}", last_mark)?;
                if opts.show_original_ids {
                    writeln!(out, "original-oid {}", name)?;
                }
                writeln!(out, "data {}", data.len())?;
                out.write_all(&data)?;
                out.write_all(b"\n")?;
            }
        }

        last_mark += 1;
        marks.insert(hash.to_string(), last_mark);
        let refname = source.get(hash).cloned().unwrap_or_else(|| hash.to_string());
        if parents.is_empty() {
            writeln!(out, "reset {}", refname)?;
        }
        writeln!(out, "commit {}\nmark :{}", refname, last_mark)?;
        if opts.show_original_ids {
            writeln!(out, "original-oid {}", hash)?;
        }
        let committer = field("committer ").unwrap_or_default();
        writeln!(out, "author {}\ncommitter {}", field("author ").unwrap_or_else(|| committer.clone()), committer)?;
        if let Some(encoding) = field("encoding ") {
            writeln!(out, "encoding {}", encoding)?;
        }
        writeln!(out, "data {}", message.len())?;
        out.write_all(&message)?;
        let mut listed = 0;
        for parent in &parents {
            let reference = match marks.get(parent) {
                Some(mark) => format!(":{}", mark),
                None if opts.reference_excluded => parent.clone(),
                None => continue,
            };
            writeln!(out, "{} {}", if listed == 0 { "from" } else { "merge" }, reference)?;
            listed += 1;
        }
        if opts.full_tree {
            writeln!(out, "deleteall")?;
        }
        // git's order (depth_first): by the path the change starts from, a path before its own
        // prefix (`d/e` before `d`), renames last among equals.
        let key = |c: &DiffChange| c.renamed_from.as_ref().map(|r| r.0.clone()).unwrap_or_else(|| c.path.clone());
        diff.sort_by(|a, b| {
            let (ka, kb) = (key(a), key(b));
            let (x, y) = (ka.as_bytes(), kb.as_bytes());
            let common = x.len().min(y.len());
            x[..common]
                .cmp(&y[..common])
                .then(y.len().cmp(&x.len()))
                .then((a.renamed_from.as_ref().map(|r| r.1) == Some('R')).cmp(&(b.renamed_from.as_ref().map(|r| r.1) == Some('R'))))
        });
        // A rename or copy is only declared when its source was not changed earlier in this
        // commit; otherwise (and when the content differs) the file is written out with `M`.
        let mut changed: BTreeSet<String> = BTreeSet::new();
        for change in &diff {
            if let Some((source, status, _)) = &change.renamed_from
                && !changed.contains(source)
            {
                writeln!(out, "{} {} {}", status, export_path(source), export_path(&change.path))?;
                changed.insert(change.path.clone());
                if change.old == change.new {
                    continue;
                }
            }
            match &change.new {
                None => writeln!(out, "D {}", export_path(&change.path))?,
                Some(new) => {
                    let name = hex::encode(new.hash);
                    let reference = match marks.get(&name) {
                        Some(mark) if !opts.no_data && new.mode != 0o160000 => format!(":{}", mark),
                        _ => name,
                    };
                    writeln!(out, "M {:06o} {} {}", new.mode, reference, export_path(&change.path))?;
                }
            }
            changed.insert(change.path.clone());
        }
        writeln!(out)?;
    }

    // Other refs on exported commits, last one first, as git does.
    for (name, commit) in extra_refs.iter().rev() {
        match marks.get(commit) {
            Some(mark) => writeln!(out, "reset {}\nfrom :{}\n", name, mark)?,
            None if opts.reference_excluded => writeln!(out, "reset {}\nfrom {}\n", name, commit)?,
            None => writeln!(out, "reset {}\nfrom {}\n", name, "0".repeat(40))?,
        }
    }
    for (name, tag_hash) in tag_refs.iter().rev() {
        let (_, content) = read_object(tag_hash)?;
        let (header, mut message) = split_object(&content);
        let target = header.iter().find_map(|l| l.strip_prefix("object ")).unwrap_or("").to_string();
        let short = name.strip_prefix("refs/tags/").unwrap_or(name);
        if let Some(at) = message.windows(27).position(|w| w == b"-----BEGIN PGP SIGNATURE---") {
            match opts.signed_tags.as_str() {
                "abort" => bail!("fatal: encountered signed tag {}; use --signed-tags=<mode> to handle it", tag_hash),
                "warn" => eprintln!("warning: exporting signed tag {}", tag_hash),
                "warn-strip" => {
                    eprintln!("warning: stripping signature from tag {}", tag_hash);
                    message.truncate(at);
                }
                "strip" => message.truncate(at),
                _ => {}
            }
        }
        let reference = match marks.get(&target) {
            Some(mark) => format!(":{}", mark),
            None if opts.tag_of_filtered == "drop" => continue,
            None => bail!("fatal: tag {} tags unexported object; use --tag-of-filtered-object=<mode> to handle it", tag_hash),
        };
        writeln!(out, "tag {}", short)?;
        if opts.mark_tags {
            last_mark += 1;
            marks.insert(tag_hash.clone(), last_mark);
            writeln!(out, "mark :{}", last_mark)?;
        }
        writeln!(out, "from {}", reference)?;
        if opts.show_original_ids {
            writeln!(out, "original-oid {}", tag_hash)?;
        }
        if let Some(tagger) = header.iter().find_map(|l| l.strip_prefix("tagger ")) {
            writeln!(out, "tagger {}", tagger)?;
        }
        writeln!(out, "data {}", message.len())?;
        out.write_all(&message)?;
        out.write_all(b"\n")?;
    }
    if opts.use_done {
        writeln!(out, "done")?;
    }

    if let Some(file) = &opts.export_marks {
        // Like git, only commits and tags are recorded, not blobs.
        let mut listed: Vec<(&u32, &String)> = marks.iter().filter(|(hash, _)| !exported_blobs.contains(*hash)).map(|(hash, mark)| (mark, hash)).collect();
        listed.sort();
        let text: String = listed.iter().map(|(mark, hash)| format!(":{} {}\n", mark, hash)).collect();
        std::fs::write(file, text)?;
    }
    Ok(())
}
