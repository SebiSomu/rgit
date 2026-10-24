use std::collections::{BTreeMap, HashSet};
use std::io::{self, BufWriter, Write};
use anyhow::Result;

use crate::helpers::bisect::collect_ancestors;
use crate::helpers::format_patch::{parse_commit_object, split_commit_message};
use crate::helpers::merge_tree::{merge_base_tree, merge_trees_in_memory};
use crate::helpers::rev_parse::{peel_to_type, resolve_rev};
use crate::helpers::{build_tree_from_map, flatten_tree};
use crate::objects::{MergePathOutcome, MergeTreeOptions, TreeMergeResult};

const PREVIEW_LINES: usize = 6;

/// `rgit merge-tree [--write-tree] [--name-only] [--[no-]messages] [-z] [--merge-base=<tree-ish>]
/// [--allow-unrelated-histories] [--explain] <branch1> <branch2>`
///
/// Merges `<branch2>` into `<branch1>` entirely in memory and writes the merged tree (with
/// conflict markers in conflicted files) to the object store, leaving the index, working tree,
/// and refs untouched. Prints git's output: the tree hash, then on conflicts the conflicted
/// paths with their stages (`<mode> <object> <stage>\t<path>`, or just names with
/// `--name-only`), a blank line, and the merge messages; `-z` switches to NUL-terminated,
/// machine-readable records. `--explain` instead prints a readable report of the simulation:
/// the commits and merge base involved, what each side did to every changed path and what the
/// merge decided, and a side-by-side preview of every conflicting region. Exits 0 for a clean
/// merge, 1 for conflicts, and 128 on errors.
pub fn merge_tree(opts: MergeTreeOptions) -> Result<()> {
    match run_merge_tree(&opts) {
        Ok(clean) => std::process::exit(if clean { 0 } else { 1 }),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(128);
        }
    }
}

fn run_merge_tree(opts: &MergeTreeOptions) -> Result<bool> {
    let resolve = |name: &str, kind: &str| -> String {
        resolve_rev(name).and_then(|h| peel_to_type(h, kind)).unwrap_or_else(|_| {
            eprintln!("merge-tree: {} - not something we can merge", name);
            std::process::exit(1);
        })
    };
    let tree_of = |hash: &str| -> Result<BTreeMap<String, ([u8; 20], u32)>> {
        let mut tree = BTreeMap::new();
        flatten_tree(&peel_to_type(hash.to_string(), "tree")?, "", &mut tree)?;
        Ok(tree)
    };

    let kind = if opts.merge_base.is_some() { "tree" } else { "commit" };
    let ours = resolve(&opts.branch1, kind);
    let theirs = resolve(&opts.branch2, kind);
    let (base_tree, bases) = match &opts.merge_base {
        Some(base) => (tree_of(&resolve(base, "tree"))?, Vec::new()),
        None => merge_base_tree(&ours, &theirs, opts.allow_unrelated_histories)?,
    };

    let result = merge_trees_in_memory(&base_tree, &tree_of(&ours)?, &tree_of(&theirs)?, &opts.branch1, &opts.branch2, 7)?;
    let tree_hash = build_tree_from_map(&result.tree)?;
    let clean = result.conflicted.is_empty();

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if opts.explain {
        write_explanation(&mut out, opts, &ours, &theirs, &bases, &result, &tree_hash)?;
    } else {
        write_git_output(&mut out, opts, &result, &tree_hash)?;
    }
    out.flush()?;
    Ok(clean)
}

fn write_git_output(out: &mut impl Write, opts: &MergeTreeOptions, result: &TreeMergeResult, tree_hash: &str) -> Result<()> {
    let end = if opts.nul { '\0' } else { '\n' };
    let clean = result.conflicted.is_empty();
    write!(out, "{}{}", tree_hash, end)?;
    if !clean {
        for (path, stages) in &result.conflicted {
            if opts.name_only {
                write!(out, "{}{}", path, end)?;
                continue;
            }
            for (stage, entry) in stages.iter().enumerate() {
                if let Some((hash, mode)) = entry {
                    write!(out, "{:06o} {} {}\t{}{}", mode, hex::encode(hash), stage + 1, path, end)?;
                }
            }
        }
    }
    if !opts.messages.unwrap_or(!clean) {
        return Ok(());
    }
    write!(out, "{}", end)?;
    for message in &result.messages {
        if opts.nul {
            write!(out, "{}\0", message.paths.len())?;
            for path in &message.paths {
                write!(out, "{}\0", path)?;
            }
            write!(out, "{}\0{}\n\0", message.kind, message.text)?;
        } else {
            writeln!(out, "{}", message.text)?;
        }
    }
    Ok(())
}

fn commits_since(tip: &str, bases: &[String]) -> Result<usize> {
    let mut excluded = HashSet::new();
    for base in bases {
        excluded.extend(collect_ancestors(base)?);
    }
    Ok(collect_ancestors(tip)?.difference(&excluded).count())
}

fn describe_commit(hash: &str) -> String {
    match parse_commit_object(hash) {
        Ok(c) => format!("{}  {}", &hash[..7], split_commit_message(&c.message).0),
        Err(_) => hash[..7].to_string(),
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{} {}{}", n, word, if n == 1 { "" } else { "s" })
}

fn write_explanation(out: &mut impl Write, opts: &MergeTreeOptions, ours: &str, theirs: &str, bases: &[String], result: &TreeMergeResult, tree_hash: &str) -> Result<()> {
    let (l1, l2) = (opts.branch1.as_str(), opts.branch2.as_str());
    let width = l1.len().max(l2.len()).max(4);
    writeln!(out, "Merge simulation: {} into {}", l2, l1)?;
    writeln!(out, "(dry run: only objects are written; refs, index and working tree are untouched)")?;
    writeln!(out)?;

    if opts.merge_base.is_some() {
        writeln!(out, "  ours    {:<width$}  {}", l1, &ours[..7])?;
        writeln!(out, "  theirs  {:<width$}  {}", l2, &theirs[..7])?;
        writeln!(out, "  base    {:<width$}  {}  (given with --merge-base)", "", opts.merge_base.as_deref().unwrap_or_default())?;
    } else {
        let since = |tip: &str| -> Result<String> {
            Ok(if bases.is_empty() { "no shared history".into() } else { format!("{} since base", plural(commits_since(tip, bases)?, "commit")) })
        };
        let (ours_desc, theirs_desc) = (describe_commit(ours), describe_commit(theirs));
        let desc_width = ours_desc.chars().count().max(theirs_desc.chars().count());
        writeln!(out, "  ours    {:<width$}  {:<desc_width$}   [{}]", l1, ours_desc, since(ours)?)?;
        writeln!(out, "  theirs  {:<width$}  {:<desc_width$}   [{}]", l2, theirs_desc, since(theirs)?)?;
        match bases {
            [] => writeln!(out, "  base    {:<width$}  (empty tree: --allow-unrelated-histories)", "")?,
            [base] => writeln!(out, "  base    {:<width$}  {}", "", describe_commit(base))?,
            _ => {
                let names: Vec<&str> = bases.iter().map(|b| &b[..7]).collect();
                writeln!(out, "  base    {:<width$}  virtual: {} merge bases ({}) merged together first", "", bases.len(), names.join(", "))?;
            }
        }
    }
    writeln!(out)?;

    if result.reports.is_empty() {
        writeln!(out, "Neither side changed anything since the merge base.")?;
    } else {
        let path_width = result.reports.iter().map(|r| r.path.len()).max().unwrap_or(4).max(4);
        let side_width = width.max(8);
        writeln!(out, "  {:<path_width$}  {:<side_width$}  {:<side_width$}  RESULT", "PATH", l1, l2)?;
        for report in &result.reports {
            let (mark, decision) = match &report.outcome {
                MergePathOutcome::SameOnBothSides => ("=", "same change on both sides".to_string()),
                MergePathOutcome::TakenFromOurs => ("<", format!("kept {}'s version", l1)),
                MergePathOutcome::TakenFromTheirs => (">", format!("took {}'s version", l2)),
                MergePathOutcome::AutoMerged => {
                    let f = report.file.as_ref().unwrap();
                    let mut parts = vec![format!("{} from {}", plural(f.from_ours, "hunk"), l1), format!("{} from {}", f.from_theirs, l2)];
                    if f.from_both > 0 {
                        parts.push(format!("{} identical", f.from_both));
                    }
                    ("+", format!("auto-merged ({})", parts.join(", ")))
                }
                MergePathOutcome::Conflict(kind) => {
                    let detail = match (kind.as_str(), &report.file) {
                        (_, Some(f)) if !f.conflicts.is_empty() => {
                            let clean = f.from_ours + f.from_theirs + f.from_both;
                            let extra = if clean > 0 { format!(" (+{} merged cleanly)", plural(clean, "hunk")) } else { String::new() };
                            format!(": {}{}", plural(f.conflicts.len(), "conflicting region"), extra)
                        }
                        ("modify/delete", _) => {
                            let kept = if report.ours == crate::objects::SideChange::Deleted { l2 } else { l1 };
                            format!(": {}'s modified version left in tree", kept)
                        }
                        ("binary", _) => format!(": can't merge binary content, kept {}'s version", l1),
                        ("file/directory", _) => {
                            let prefix = format!("{}~", report.path);
                            let moved = result.conflicted.keys().find(|k| k.starts_with(&prefix)).cloned().unwrap_or(prefix);
                            format!(": a directory is in the way, file moved to {}", moved)
                        }
                        _ => String::new(),
                    };
                    ("!", format!("CONFLICT ({}){}", kind, detail))
                }
            };
            writeln!(out, "{} {:<path_width$}  {:<side_width$}  {:<side_width$}  {}", mark, report.path, report.ours.label(), report.theirs.label(), decision)?;
        }
        writeln!(out)?;
        writeln!(out, "  legend: < kept ours   > took theirs   = same on both   + merged line by line   ! conflict")?;
    }

    let regions: Vec<_> = result.reports.iter().filter_map(|r| r.file.as_ref().map(|f| (r, f))).filter(|(_, f)| !f.conflicts.is_empty()).collect();
    if !regions.is_empty() {
        writeln!(out)?;
        writeln!(out, "Conflicting regions (what each side wants in place of the base lines):")?;
        for (report, file) in regions {
            for (n, region) in file.conflicts.iter().enumerate() {
                let range = match region.base.len() {
                    0 if region.base_line == 1 => "insertion at the start of the file".to_string(),
                    0 => format!("insertion after base line {}", region.base_line - 1),
                    1 => format!("base line {}", region.base_line),
                    len => format!("base lines {}-{}", region.base_line, region.base_line + len - 1),
                };
                writeln!(out)?;
                writeln!(out, "  -- {}  [{}/{}]  {} --", report.path, n + 1, file.conflicts.len(), range)?;
                for (label, lines) in [("base", &region.base), (l1, &region.ours), (l2, &region.theirs)] {
                    if lines.is_empty() {
                        writeln!(out, "     {:<width$} | (nothing)", label)?;
                    }
                    for line in lines.iter().take(PREVIEW_LINES) {
                        writeln!(out, "     {:<width$} | {}", label, line.trim_end_matches(['\n', '\r']))?;
                    }
                    if lines.len() > PREVIEW_LINES {
                        writeln!(out, "     {:<width$} | ... ({} more lines)", label, lines.len() - PREVIEW_LINES)?;
                    }
                }
            }
        }
    }

    writeln!(out)?;
    let conflicts = result.conflicted.len();
    if conflicts == 0 {
        writeln!(out, "Result: clean merge of {}.", plural(result.reports.len(), "changed path"))?;
    } else {
        writeln!(out, "Result: {} out of {} would need manual resolution.", plural(conflicts, "conflicted path"), plural(result.reports.len(), "changed path"))?;
    }
    writeln!(out, "Merged tree {} (conflicted files contain markers). Inspect with `rgit ls-tree {}`.", tree_hash, tree_hash)?;
    Ok(())
}
