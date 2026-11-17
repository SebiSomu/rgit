use std::collections::BTreeMap;

use crate::helpers::cherry::abbreviate_hash_min;
use crate::helpers::format_patch::{diff_file_contents, format_diffstat};
use crate::helpers::index_plumbing::worktree_state;
use crate::helpers::ls_files::pathspec_wildcard_match;
use crate::helpers::objects::{hash_content, read_object};
use crate::index::IndexEntry;
use crate::objects::{DiffChange, DiffPlumbingOptions, DiffSide, DiffStatEntry, FileDiff, IndexFileState};

/// The options every raw diff command understands, for the usage text.
pub const DIFF_COMMON_OPTIONS: &str = "common diff options:\n  -z            output diff-raw with lines terminated with NUL.\n  -p            output patch format.\n  -u            synonym for -p.\n  --raw         output diff-raw (the default).\n  --name-only   show only names of changed files.\n  --name-status show names and status of changed files.\n  --stat        show a diffstat.\n  --shortstat   show only the last line of the diffstat.\n  --numstat     show added and deleted line counts per file.\n  --summary     show creations, deletions and mode changes.\n  -s            no diff output.\n  --abbrev[=<n>] abbreviate object names in diff-raw.\n  -R            swap input file pairs.\n  --exit-code   exit with 1 if there were differences.\n  --quiet       no output, implies --exit-code.\n";

/// Parses the arguments of a raw diff command (`command` is its name, for messages).
/// Options may come anywhere before `--`. `Err` is `(text, exit status)`.
// Used by `diff-tree`, `diff-index` and `diff-files`.
pub fn parse_diff_plumbing_args(args: &[String], command: &str, usage: &str) -> Result<DiffPlumbingOptions, (String, i32)> {
    let mut opts = DiffPlumbingOptions::default();
    // The porcelain `diff` detects renames unless told otherwise (`diff.renames`).
    if command == "diff" {
        opts.renames = crate::helpers::rename::default_rename_options("diff.renames");
        // Porcelain raw output abbreviates object names.
        opts.abbrev = Some(7);
    }
    let unsupported = |option: &str, why: &str| (format!("fatal: {} is not supported by rgit {} ({})\n", option, command, why), 128);
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if arg == "--" {
            opts.paths.extend(args[i..].iter().cloned());
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            opts.args.push(arg.to_string());
            continue;
        }
        match arg {
            "-h" => return Err((usage.to_string(), 129)),
            "-p" | "-u" | "--patch" => opts.patch = true,
            "--raw" => opts.raw = true,
            "--name-only" => opts.name_only = true,
            "--name-status" => opts.name_status = true,
            "--stat" => opts.stat = true,
            "--shortstat" => opts.shortstat = true,
            "--numstat" => opts.numstat = true,
            "--summary" => opts.summary = true,
            "-s" | "--no-patch" => opts.no_output = true,
            "-z" => opts.nul = true,
            "--abbrev" => opts.abbrev = Some(7),
            "--no-abbrev" | "--full-index" => opts.abbrev = None,
            "-R" => opts.reverse = true,
            "--exit-code" => opts.exit_code = true,
            "--quiet" => opts.quiet = true,
            "--no-renames" => opts.renames.level = 0,
            "-r" if command == "diff-tree" => opts.recursive = true,
            "-t" if command == "diff-tree" => {
                opts.show_trees = true;
                opts.recursive = true;
            }
            "--root" if command == "diff-tree" => opts.root = true,
            "--stdin" if command == "diff-tree" => opts.stdin = true,
            "--no-commit-id" if command == "diff-tree" => opts.no_commit_id = true,
            "-v" | "--pretty" if command == "diff-tree" => opts.pretty = Some("medium".to_string()),
            "-m" if command != "diff-files" => opts.merges = true,
            "--cached" if command == "diff-index" || command == "diff" => opts.cached = true,
            "--staged" if command == "diff" => opts.cached = true,
            "-q" if command == "diff-files" => {}
            "-c" | "--cc" | "--combined-all-paths" => return Err(unsupported(arg, "no combined diffs")),
            other => {
                if let Some(n) = other.strip_prefix("--abbrev=") {
                    let n: usize = n.parse().map_err(|_| (format!("error: option `abbrev' expects a numerical value\n{}", usage), 129))?;
                    opts.abbrev = Some(n.clamp(4, 40));
                } else if let Some(format) = other.strip_prefix("--pretty=").or_else(|| other.strip_prefix("--format=")).filter(|_| command == "diff-tree") {
                    opts.pretty = Some(if other.starts_with("--format=") { format!("format:{}", format) } else { format.to_string() });
                } else if crate::helpers::rename::parse_rename_option(other, &mut opts.renames) {
                } else if other.starts_with("-B") || other.starts_with("--break-rewrites") {
                    return Err(unsupported(other, "no rewrite breaking"));
                } else {
                    return Err((usage.to_string(), 129));
                }
            }
        }
    }
    Ok(opts)
}

/// Parses a raw tree object into `(mode, name, hash)` entries, in the tree's own order.
// Used by `diff_trees`.
fn read_tree_entries(tree_hash: &str) -> anyhow::Result<Vec<(u32, String, [u8; 20])>> {
    let (object_type, content) = read_object(tree_hash)?;
    if object_type != "tree" {
        anyhow::bail!("fatal: {} is not a tree", tree_hash);
    }
    let mut entries = Vec::new();
    let mut pos = 0;
    while pos < content.len() {
        let space = content[pos..].iter().position(|b| *b == b' ').map(|i| pos + i).ok_or_else(|| anyhow::anyhow!("fatal: corrupt tree {}", tree_hash))?;
        let nul = content[space..].iter().position(|b| *b == 0).map(|i| space + i).ok_or_else(|| anyhow::anyhow!("fatal: corrupt tree {}", tree_hash))?;
        let mode = u32::from_str_radix(&String::from_utf8_lossy(&content[pos..space]), 8)?;
        let name = String::from_utf8_lossy(&content[space + 1..nul]).to_string();
        let mut hash = [0u8; 20];
        hash.copy_from_slice(content.get(nul + 1..nul + 21).ok_or_else(|| anyhow::anyhow!("fatal: corrupt tree {}", tree_hash))?);
        entries.push((mode, name, hash));
        pos = nul + 21;
    }
    Ok(entries)
}

/// Whether `path` is selected by the pathspecs. For a directory (`is_tree`), a pathspec that
/// points somewhere below it also counts, so the walk can go on (or show the directory).
// Used by `diff_trees`, `diff_index_changes` and `diff_files_changes`.
pub fn diff_path_selected(path: &str, is_tree: bool, specs: &[String]) -> bool {
    specs.is_empty()
        || specs.iter().any(|spec| {
            let wild = spec.contains(['*', '?', '[']);
            spec.is_empty()
                || path == spec
                || path.starts_with(&format!("{}/", spec))
                || (is_tree && (wild || spec.starts_with(&format!("{}/", path))))
                || (wild && pathspec_wildcard_match(spec.as_bytes(), path.as_bytes()))
        })
}

/// True for the mode of a directory entry (`040000`).
// Used by `diff_trees` and `change_patch`.
fn is_tree_mode(mode: u32) -> bool {
    mode & 0o170000 == 0o040000
}

/// Compares two trees (either may be absent) and appends the changed entries below `prefix`,
/// in git's order. Without `recursive` a changed subtree is one entry with mode `040000`;
/// with it the walk descends, and `show_trees` reports the subtree entry as well.
// Used by `diff-tree`.
pub fn diff_trees(old: Option<&str>, new: Option<&str>, prefix: &str, opts: &DiffPlumbingOptions, specs: &[String], out: &mut Vec<DiffChange>) -> anyhow::Result<()> {
    let load = |tree: Option<&str>| -> anyhow::Result<BTreeMap<Vec<u8>, (u32, String, [u8; 20])>> {
        let mut map = BTreeMap::new();
        if let Some(tree) = tree {
            for (mode, name, hash) in read_tree_entries(tree)? {
                // git orders tree entries as if directory names ended in a slash.
                let mut key = name.clone().into_bytes();
                if is_tree_mode(mode) {
                    key.push(b'/');
                }
                map.insert(key, (mode, name, hash));
            }
        }
        Ok(map)
    };
    let (old_entries, new_entries) = (load(old)?, load(new)?);
    let keys: std::collections::BTreeSet<&Vec<u8>> = old_entries.keys().chain(new_entries.keys()).collect();
    let side = |entry: &(u32, String, [u8; 20])| DiffSide { mode: entry.0, hash: entry.2, worktree: false };

    for key in keys {
        let (a, b) = (old_entries.get(key), new_entries.get(key));
        let Some(entry) = a.or(b) else { continue };
        let path = format!("{}{}", prefix, entry.1);
        let tree = is_tree_mode(entry.0);
        if !diff_path_selected(&path, tree, specs) {
            continue;
        }
        if let (Some(a), Some(b)) = (a, b) {
            if a.0 == b.0 && a.2 == b.2 {
                continue;
            }
        }
        if tree {
            if !opts.recursive || opts.show_trees {
                out.push(DiffChange { path: path.clone(), old: a.map(side), new: b.map(side), renamed_from: None });
            }
            if opts.recursive {
                let (old_sub, new_sub) = (a.map(|e| hex::encode(e.2)), b.map(|e| hex::encode(e.2)));
                diff_trees(old_sub.as_deref(), new_sub.as_deref(), &format!("{}/", path), opts, specs, out)?;
            }
        } else {
            out.push(DiffChange { path, old: a.map(side), new: b.map(side), renamed_from: None });
        }
    }
    Ok(())
}

/// The working-tree side of an index entry for a raw diff: `None` when the file is gone,
/// the entry itself when the file is unchanged, else the file's mode with a zero hash.
// Used by `diff_index_changes` and `diff_files_changes`.
fn worktree_side(entry: &IndexEntry) -> Option<DiffSide> {
    match worktree_state(entry) {
        IndexFileState::Missing => None,
        IndexFileState::Clean => Some(DiffSide { mode: entry.mode, hash: entry.hash, worktree: false }),
        IndexFileState::Modified(mode) => Some(DiffSide { mode, hash: [0; 20], worktree: true }),
    }
}

/// `diff-index`: compares a flattened tree with the index (`cached`) or with the working
/// tree as far as the index tracks it. A tracked file missing from the working tree counts
/// as deleted, or as unchanged with `ignore_missing` (`-m`).
// Used by `diff-index`.
pub fn diff_index_changes(tree: &BTreeMap<String, ([u8; 20], u32)>, index: &[IndexEntry], cached: bool, ignore_missing: bool, specs: &[String]) -> Vec<DiffChange> {
    let by_path: BTreeMap<&str, &IndexEntry> = index.iter().map(|e| (e.path.as_str(), e)).collect();
    let paths: std::collections::BTreeSet<&str> = tree.keys().map(String::as_str).chain(by_path.keys().copied()).collect();
    let mut changes = Vec::new();
    for path in paths {
        if !diff_path_selected(path, false, specs) {
            continue;
        }
        let old = tree.get(path).map(|(hash, mode)| DiffSide { mode: *mode, hash: *hash, worktree: false });
        let new = by_path.get(path).and_then(|entry| {
            let staged = DiffSide { mode: entry.mode, hash: entry.hash, worktree: false };
            if cached {
                return Some(staged);
            }
            match worktree_side(entry) {
                None if ignore_missing => Some(staged),
                side => side,
            }
        });
        if old != new {
            changes.push(DiffChange { path: path.to_string(), old, new, renamed_from: None });
        }
    }
    changes
}

/// `diff-files`: compares the index with the working tree.
// Used by `diff-files`.
pub fn diff_files_changes(index: &[IndexEntry], specs: &[String]) -> Vec<DiffChange> {
    let mut changes = Vec::new();
    for entry in index {
        if !diff_path_selected(&entry.path, false, specs) {
            continue;
        }
        let old = DiffSide { mode: entry.mode, hash: entry.hash, worktree: false };
        let new = worktree_side(entry);
        if new.as_ref() != Some(&old) {
            changes.push(DiffChange { path: entry.path.clone(), old: Some(old), new, renamed_from: None });
        }
    }
    changes
}

/// The status letter of a change: `A`dded, `D`eleted, `T`ype changed, else `M`odified.
// Used by `render_diff_changes`.
fn status_letter(change: &DiffChange) -> char {
    if let Some((_, status, _)) = &change.renamed_from {
        return *status;
    }
    match (&change.old, &change.new) {
        (None, _) => 'A',
        (_, None) => 'D',
        (Some(a), Some(b)) if a.mode & 0o170000 != b.mode & 0o170000 => 'T',
        _ => 'M',
    }
}

/// The status as raw and `--name-status` output show it: the letter, with the similarity
/// percentage for a rename or copy (`R086`).
// Used by `render_diff_changes`.
pub fn status_field(change: &DiffChange) -> String {
    match &change.renamed_from {
        Some((_, status, score)) => format!("{}{:03}", status, u64::from(*score) * 100 / crate::helpers::rename::MAX_SCORE),
        None => status_letter(change).to_string(),
    }
}

/// The paths a raw or `--name-status` line shows: the path, or source and destination of a
/// rename or copy, separated by TAB (NUL with `-z`).
fn shown_paths(change: &DiffChange, nul: bool) -> String {
    let show = |p: &str| if nul { p.to_string() } else { quote_c_path(p) };
    match &change.renamed_from {
        Some((source, _, _)) => format!("{}{}{}", show(source), if nul { "\0" } else { "\t" }, show(&change.path)),
        None => show(&change.path),
    }
}

/// Quotes a path for line-oriented output the way git does: unchanged when it is plain,
/// otherwise in double quotes with C escapes and octal for control and non-ASCII bytes.
// Used by `render_diff_changes`.
pub fn quote_c_path(path: &str) -> String {
    let needs_quoting = path.bytes().any(|b| b < 0x20 || b == 0x7f || b == b'"' || b == b'\\' || b >= 0x80);
    if !needs_quoting {
        return path.to_string();
    }
    let mut out = String::from("\"");
    for b in path.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x07 => out.push_str("\\a"),
            0x08 => out.push_str("\\b"),
            0x0b => out.push_str("\\v"),
            0x0c => out.push_str("\\f"),
            b if b < 0x20 || b >= 0x7f => out.push_str(&format!("\\{:03o}", b)),
            b => out.push(b as char),
        }
    }
    out.push('"');
    out
}

/// The content of one side of a change, and the `(hash, mode)` a patch header shows for it.
// Used by `change_patch`.
fn side_content(path: &str, side: &Option<DiffSide>) -> anyhow::Result<(Option<([u8; 20], u32)>, Vec<u8>)> {
    match side {
        None => Ok((None, Vec::new())),
        Some(side) if side.worktree && side.mode & 0o170000 == 0o160000 => {
            let bytes = crate::helpers::submodule::submodule_worktree_bytes(path).unwrap_or_default();
            Ok((Some((side.hash, side.mode)), bytes))
        }
        Some(side) if side.worktree => {
            let content = std::fs::read(path).unwrap_or_default();
            Ok((Some((hash_content("blob", &content), side.mode)), content))
        }
        // A submodule link shows as git's one-line pseudo file.
        Some(side) if side.mode & 0o170000 == 0o160000 => Ok((Some((side.hash, side.mode)), format!("Subproject commit {}\n", hex::encode(side.hash)).into_bytes())),
        Some(side) => Ok((Some((side.hash, side.mode)), read_object(&hex::encode(side.hash))?.1)),
    }
}

/// The patch sections of one change: one `diff --git` section, or a deletion plus a creation
/// when the file type changed. Tree entries have no patch.
// Used by `render_diff_changes`.
fn change_patch(change: &DiffChange) -> anyhow::Result<Vec<FileDiff>> {
    let is_tree = |side: &Option<DiffSide>| side.as_ref().is_some_and(|s| is_tree_mode(s.mode));
    if is_tree(&change.old) || is_tree(&change.new) {
        return Ok(Vec::new());
    }
    if let Some((source, status, score)) = &change.renamed_from {
        let (old, old_bytes) = side_content(source, &change.old)?;
        let (new, new_bytes) = side_content(&change.path, &change.new)?;
        return Ok(vec![renamed_file_diff(source, &change.path, *status, *score, old, old_bytes, new, new_bytes)?]);
    }
    let (old, old_bytes) = side_content(&change.path, &change.old)?;
    let (new, new_bytes) = side_content(&change.path, &change.new)?;
    if status_letter(change) == 'T' {
        return Ok(vec![
            diff_file_contents(&change.path, old, None, old_bytes, Vec::new())?,
            diff_file_contents(&change.path, None, new, Vec::new(), new_bytes)?,
        ]);
    }
    Ok(vec![diff_file_contents(&change.path, old, new, old_bytes, new_bytes)?])
}

/// The patch of a rename or copy as git writes it: `diff --git a/<old> b/<new>`, mode lines,
/// `similarity index`, `rename from`/`rename to` (or `copy`), then the `index` line and hunks
/// when the content changed; the stat names it `old => new` (git's `pprint_rename`) and the
/// summary says ` rename <names> (N%)`, plus ` mode change` without a path.
// Used by `render_diff_changes`, `log`/`show` and `log --follow`.
#[allow(clippy::too_many_arguments)]
pub fn renamed_file_diff(source: &str, path: &str, status: char, score: u32, old: Option<([u8; 20], u32)>, old_bytes: Vec<u8>, new: Option<([u8; 20], u32)>, new_bytes: Vec<u8>) -> anyhow::Result<FileDiff> {
    // An unchanged binary file still counts as binary in --stat / --numstat.
    let unchanged_binary = old_bytes == new_bytes && old_bytes[..old_bytes.len().min(8000)].contains(&0);
    let mut diff = diff_file_contents(path, old, new, old_bytes, new_bytes)?;
    if unchanged_binary {
        diff.stat.binary_sizes = Some((0, 0));
    }
    let word = if status == 'R' { "rename" } else { "copy" };
    let percent = u64::from(score) * 100 / crate::helpers::rename::MAX_SCORE;
    let mut lines: Vec<String> = diff.text.split_inclusive('\n').map(str::to_string).collect();
    lines[0] = format!("diff --git a/{} b/{}\n", source, path);
    let after_modes = lines.iter().position(|l| !(l.starts_with("diff --git") || l.starts_with("old mode ") || l.starts_with("new mode "))).unwrap_or(lines.len());
    let rename_lines = [format!("similarity index {}%\n", percent), format!("{} from {}\n", word, source), format!("{} to {}\n", word, path)];
    lines.splice(after_modes..after_modes, rename_lines);
    for line in lines.iter_mut() {
        if *line == format!("--- a/{}\n", path) {
            *line = format!("--- a/{}\n", source);
            break;
        }
    }
    diff.text = lines.concat();
    let names = crate::helpers::rename::pprint_rename(source, path);
    diff.stat.name = names.clone();
    let mut summary = format!(" {} {} ({}%)\n", word, names, percent);
    if let (Some((_, old_mode)), Some((_, new_mode))) = (old, new)
        && old_mode != new_mode
    {
        summary.push_str(&format!(" mode change {:06o} => {:06o}\n", old_mode, new_mode));
    }
    diff.summary = Some(summary);
    Ok(diff)
}

/// Formats changes the way the raw diff commands print them: raw lines
/// (`:<old mode> <new mode> <old hash> <new hash> <status> TAB <path>`) by default, or
/// names, name and status, diffstat, numstat, summary and/or the patch. With several
/// formats the patch comes last, after a blank line. `-z` ends records with NUL and never
/// quotes paths; `-R` has already been applied by the caller via `reverse_changes`.
// Used by `diff-tree`, `diff-index` and `diff-files`.
pub fn render_diff_changes(changes: &[DiffChange], opts: &DiffPlumbingOptions) -> anyhow::Result<String> {
    if opts.no_output || opts.quiet || changes.is_empty() {
        return Ok(String::new());
    }
    let explicit = opts.patch || opts.name_only || opts.name_status || opts.stat || opts.shortstat || opts.numstat || opts.summary;
    let raw = opts.raw || !explicit;
    let end = if opts.nul { "\0" } else { "\n" };
    let shown_path = |path: &str| if opts.nul { path.to_string() } else { quote_c_path(path) };
    let shown_hash = |side: &Option<DiffSide>| -> String {
        let full = side.as_ref().map(|s| hex::encode(s.hash)).unwrap_or_else(|| "0".repeat(40));
        match opts.abbrev {
            None => full,
            Some(n) if full.bytes().all(|b| b == b'0') => "0".repeat(n),
            Some(n) => abbreviate_hash_min(&full, n),
        }
    };

    let mut out = String::new();
    if opts.name_only {
        for change in changes {
            out.push_str(&format!("{}{}", shown_path(&change.path), end));
        }
    } else if opts.name_status {
        for change in changes {
            let separator = if opts.nul { "\0" } else { "\t" };
            out.push_str(&format!("{}{}{}{}", status_field(change), separator, shown_paths(change, opts.nul), end));
        }
    } else if raw {
        for change in changes {
            let mode = |side: &Option<DiffSide>| side.as_ref().map(|s| s.mode).unwrap_or(0);
            let separator = if opts.nul { "\0" } else { "\t" };
            out.push_str(&format!(
                ":{:06o} {:06o} {} {} {}{}{}{}",
                mode(&change.old),
                mode(&change.new),
                shown_hash(&change.old),
                shown_hash(&change.new),
                status_field(change),
                separator,
                shown_paths(change, opts.nul),
                end
            ));
        }
    }

    if !(opts.patch || opts.stat || opts.shortstat || opts.numstat || opts.summary) {
        return Ok(out);
    }
    let mut diffs: Vec<FileDiff> = Vec::new();
    for change in changes {
        diffs.extend(change_patch(change)?);
    }
    if opts.numstat {
        for diff in &diffs {
            match diff.stat.binary_sizes {
                Some(_) => out.push_str(&format!("-\t-\t{}{}", shown_path(&diff.stat.name), end)),
                None => out.push_str(&format!("{}\t{}\t{}{}", diff.stat.added, diff.stat.deleted, shown_path(&diff.stat.name), end)),
            }
        }
    }
    if (opts.stat || opts.shortstat) && !diffs.is_empty() {
        let stats: Vec<DiffStatEntry> = diffs.iter().map(|d| d.stat.clone()).collect();
        let block = format_diffstat(&stats, if opts.stat_width == 0 { 80 } else { opts.stat_width });
        if opts.stat {
            out.push_str(&block);
        } else if let Some(last) = block.lines().last() {
            out.push_str(last);
            out.push('\n');
        }
    }
    if opts.summary {
        for diff in &diffs {
            if let Some(summary) = &diff.summary {
                out.push_str(summary);
            }
        }
    }
    if opts.patch {
        if !out.is_empty() {
            out.push('\n');
        }
        for (diff, change) in diffs.iter().zip(changes.iter().flat_map(|c| std::iter::repeat_n(c, if status_letter(c) == 'T' && c.renamed_from.is_none() { 2 } else { 1 }))) {
            // Like git without --binary: no binary patch, just "Binary files ... differ".
            let source = change.renamed_from.as_ref().map(|r| r.0.as_str()).unwrap_or(&change.path);
            let text = crate::helpers::log::binary_patch_as_notice(&diff.text, source, &change.path);
            if opts.reverse {
                out.push_str(&swap_patch_prefixes(&text));
            } else {
                out.push_str(&text);
            }
        }
    }
    Ok(out)
}

/// The files a change list leaves untouched, from the side the changes start from: the extra
/// copy sources of `--find-copies-harder`.
// Used by `diff-tree`, `diff-index`, `diff-files`, `diff`, `log` and `fast-export`.
pub fn unmodified_files(old_side: &BTreeMap<String, ([u8; 20], u32)>, changes: &[DiffChange]) -> Vec<(String, DiffSide)> {
    let changed: std::collections::HashSet<&str> = changes.iter().map(|c| c.path.as_str()).collect();
    old_side
        .iter()
        .filter(|(p, _)| !changed.contains(p.as_str()))
        .map(|(p, (hash, mode))| (p.clone(), DiffSide { mode: *mode, hash: *hash, worktree: false }))
        .collect()
}

/// Applies `-R`: swaps the two sides of every change.
// Used by `diff-tree`, `diff-index` and `diff-files`.
pub fn reverse_changes(changes: &mut [DiffChange]) {
    for change in changes {
        std::mem::swap(&mut change.old, &mut change.new);
    }
}

/// With `-R` git also swaps the `a/` and `b/` prefixes in a patch header
/// (`diff --git b/x a/x`, `--- b/x`, `+++ a/x`); this rewrites the header lines accordingly.
// Used by `render_diff_changes`.
fn swap_patch_prefixes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_header = true;
    for line in text.split_inclusive('\n') {
        if in_header && (line.starts_with("@@") || line.starts_with("GIT binary patch")) {
            in_header = false;
        }
        if !in_header {
            out.push_str(line);
        } else if let Some(rest) = line.strip_prefix("diff --git a/") {
            match rest.split_once(" b/") {
                Some((old, new)) => out.push_str(&format!("diff --git b/{} a/{}", old, new)),
                None => out.push_str(line),
            }
        } else if let Some(rest) = line.strip_prefix("--- a/") {
            out.push_str(&format!("--- b/{}", rest));
        } else if let Some(rest) = line.strip_prefix("+++ b/") {
            out.push_str(&format!("+++ a/{}", rest));
        } else {
            out.push_str(line);
        }
    }
    out
}

/// git's check for a path given without `--`: it must exist in the working tree, otherwise
/// the argument is neither a revision nor a path. Returns git's three-line `fatal:` text.
// Used by `diff-tree` and `diff-index`.
pub fn check_path_argument(arg: &str) -> Result<(), String> {
    if std::path::Path::new(arg).exists() {
        return Ok(());
    }
    Err(format!("fatal: ambiguous argument '{}': unknown revision or path not in the working tree.\nUse '--' to separate paths from revisions, like this:\n'git <command> [<revision>...] -- [<file>...]'\n", arg))
}
