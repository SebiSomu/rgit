use anyhow::{Context, Result};
use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use crate::helpers::gc::{
    check_no_pack_files, hashes_in_file, list_loose_objects, GC_LENIENT_STATE_FILES,
    GC_STRICT_STATE_FILES,
};
use crate::helpers::reflog::{list_reflog_refs, read_reflog, NULL_HASH};
use crate::index::read_index;
use crate::objects::{FsckIssue, FsckNode, FsckReport, LooseObject};

/// Modes a tree entry may have: regular file, executable, symlink, directory, submodule.
const VALID_TREE_MODES: &[&str] = &["100644", "100755", "120000", "40000", "160000"];

/// Says whether `s` is a lowercase 40-digit hexadecimal object hash.
// Used by `parse_commit_object`, `parse_tag_object` and `check_refs`.
fn is_object_hash(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// Says whether a header line looks like `author Name <email> <timestamp> <+tz>`.
// Used by `parse_commit_object`.
fn is_valid_ident_line(line: &str) -> bool {
    let (Some(open), Some(close)) = (line.find('<'), line.find('>')) else {
        return false;
    };
    if close < open {
        return false;
    }
    let mut tail = line[close + 1..].split_whitespace();
    let timestamp_ok = tail.next().is_some_and(|t| t.parse::<u64>().is_ok());
    let tz_ok = tail
        .next()
        .is_some_and(|t| t.len() == 5 && t.starts_with(['+', '-']) && t[1..].chars().all(|c| c.is_ascii_digit()));
    timestamp_ok && tz_ok
}

/// Checks a commit's headers (one valid `tree`, valid `parent`s, `author` and `committer`
/// lines, a blank line before the message) and returns the objects it links to.
// Used by `verify_loose_object`.
fn parse_commit_object(hash: &str, body: &[u8], issues: &mut Vec<FsckIssue>) -> Vec<(String, &'static str)> {
    let text = String::from_utf8_lossy(body);
    let mut lines = text.split('\n');
    let mut links = Vec::new();

    match lines.next().unwrap_or("").strip_prefix("tree ") {
        Some(tree) if is_object_hash(tree) => links.push((tree.to_string(), "tree")),
        Some(tree) => issues.push(FsckIssue::error(format!("commit {}: invalid tree hash '{}'", hash, tree))),
        None => issues.push(FsckIssue::error(format!("commit {}: first line is not a tree header", hash))),
    }

    let (mut has_author, mut has_committer, mut saw_blank) = (false, false, false);
    for line in lines {
        if line.is_empty() {
            saw_blank = true;
            break;
        }
        if let Some(parent) = line.strip_prefix("parent ") {
            if is_object_hash(parent) {
                links.push((parent.to_string(), "commit"));
            } else {
                issues.push(FsckIssue::error(format!("commit {}: invalid parent hash '{}'", hash, parent)));
            }
        } else if line.starts_with("author ") {
            has_author = true;
            if !is_valid_ident_line(line) {
                issues.push(FsckIssue::error(format!("commit {}: invalid author line", hash)));
            }
        } else if line.starts_with("committer ") {
            has_committer = true;
            if !is_valid_ident_line(line) {
                issues.push(FsckIssue::error(format!("commit {}: invalid committer line", hash)));
            }
        }
    }

    if !has_author {
        issues.push(FsckIssue::error(format!("commit {}: missing author line", hash)));
    }
    if !has_committer {
        issues.push(FsckIssue::error(format!("commit {}: missing committer line", hash)));
    }
    if !saw_blank {
        issues.push(FsckIssue::error(format!("commit {}: no blank line between headers and message", hash)));
    }
    links
}

/// Checks a tree's entries (valid modes, sane names, no duplicates, git's ordering) and
/// returns the objects it links to. Submodule entries link to other repositories, not objects.
// Used by `verify_loose_object`.
fn parse_tree_object(hash: &str, body: &[u8], issues: &mut Vec<FsckIssue>) -> Vec<(String, &'static str)> {
    let mut links = Vec::new();
    let mut names: HashSet<Vec<u8>> = HashSet::new();
    let mut previous_key: Option<Vec<u8>> = None;
    let mut out_of_order = false;
    let mut pos = 0;

    while pos < body.len() {
        let Some(space) = body[pos..].iter().position(|&b| b == b' ').map(|i| i + pos) else {
            issues.push(FsckIssue::error(format!("tree {}: truncated entry (no mode separator)", hash)));
            return links;
        };
        let Some(nul) = body[space..].iter().position(|&b| b == 0).map(|i| i + space) else {
            issues.push(FsckIssue::error(format!("tree {}: truncated entry (no name terminator)", hash)));
            return links;
        };
        if nul + 21 > body.len() {
            issues.push(FsckIssue::error(format!("tree {}: truncated entry (incomplete hash)", hash)));
            return links;
        }

        let mode = String::from_utf8_lossy(&body[pos..space]).to_string();
        let name = body[space + 1..nul].to_vec();
        let name_text = String::from_utf8_lossy(&name).to_string();
        let child = hex::encode(&body[nul + 1..nul + 21]);
        pos = nul + 21;

        let is_dir = mode == "40000" || mode == "040000";
        if mode == "040000" {
            issues.push(FsckIssue::warning(format!("tree {}: zero-padded mode 040000 on '{}'", hash, name_text)));
        } else if !VALID_TREE_MODES.contains(&mode.as_str()) {
            issues.push(FsckIssue::error(format!("tree {}: invalid mode {} on '{}'", hash, mode, name_text)));
        }

        let lowered = name_text.to_ascii_lowercase();
        if name.is_empty() {
            issues.push(FsckIssue::error(format!("tree {}: entry with an empty name", hash)));
        } else if name.contains(&b'/') {
            issues.push(FsckIssue::error(format!("tree {}: name '{}' contains a slash", hash, name_text)));
        } else if name_text == "." || name_text == ".." || lowered == ".git" {
            issues.push(FsckIssue::error(format!("tree {}: forbidden path component '{}'", hash, name_text)));
        }
        if !names.insert(name.clone()) {
            issues.push(FsckIssue::error(format!("tree {}: duplicate entry '{}'", hash, name_text)));
        }

        // Git sorts entries bytewise, comparing a directory as if its name ended in '/'.
        let mut key = name;
        if is_dir {
            key.push(b'/');
        }
        if previous_key.as_ref().is_some_and(|prev| *prev > key) {
            out_of_order = true;
        }
        previous_key = Some(key);

        match mode.as_str() {
            "160000" => {}
            "40000" | "040000" => links.push((child, "tree")),
            _ => links.push((child, "blob")),
        }
    }

    if out_of_order {
        issues.push(FsckIssue::warning(format!(
            "tree {}: entries are not in git's order (a directory sorts as if its name ended in '/'); git would compute a different hash for this tree",
            hash
        )));
    }
    links
}

/// Checks an annotated tag's `object` header and returns the object it points at.
// Used by `verify_loose_object`.
fn parse_tag_object(hash: &str, body: &[u8], issues: &mut Vec<FsckIssue>) -> Vec<(String, &'static str)> {
    let text = String::from_utf8_lossy(body);
    match text.lines().next().and_then(|l| l.strip_prefix("object ")) {
        Some(target) if is_object_hash(target) => vec![(target.to_string(), "any")],
        _ => {
            issues.push(FsckIssue::error(format!("tag {}: missing or invalid object header", hash)));
            Vec::new()
        }
    }
}

/// Checks one loose object end to end: that its zlib stream decompresses, its header names a
/// known type and the right size, and (unless `check_hash` is off) that the SHA-1 of its
/// contents is the hash it is filed under. Then it checks the contents for that type and
/// returns the node for the object graph. A damaged object still yields a node (with no links),
/// so that things pointing at it are not also reported as missing.
// Used by `run_fsck_checks`.
pub fn verify_loose_object(object: &LooseObject, check_hash: bool, issues: &mut Vec<FsckIssue>) -> FsckNode {
    let damaged = |kind: &str| FsckNode { kind: kind.to_string(), links: Vec::new() };

    let compressed = match fs::read(&object.path) {
        Ok(bytes) => bytes,
        Err(err) => {
            issues.push(FsckIssue::error(format!("object {}: cannot be read: {}", object.hash, err)));
            return damaged("unknown");
        }
    };

    let mut raw = Vec::new();
    if ZlibDecoder::new(&compressed[..]).read_to_end(&mut raw).is_err() {
        issues.push(FsckIssue::error(format!("object {}: corrupt (cannot decompress)", object.hash)));
        return damaged("unknown");
    }

    let Some(nul) = raw.iter().position(|&b| b == 0) else {
        issues.push(FsckIssue::error(format!("object {}: corrupt (no header terminator)", object.hash)));
        return damaged("unknown");
    };
    let header = String::from_utf8_lossy(&raw[..nul]).to_string();
    let body = &raw[nul + 1..];
    let (kind, declared) = header.split_once(' ').unwrap_or((header.as_str(), ""));

    if check_hash {
        let actual = hex::encode(Sha1::digest(&raw));
        if actual != object.hash {
            issues.push(FsckIssue::error(format!(
                "object {}: hash mismatch (its contents hash to {})",
                object.hash, actual
            )));
        }
    }

    if !matches!(kind, "blob" | "tree" | "commit" | "tag") {
        issues.push(FsckIssue::error(format!("object {}: unknown object type '{}'", object.hash, kind)));
        return damaged("unknown");
    }
    if declared.parse::<usize>().ok() != Some(body.len()) {
        issues.push(FsckIssue::error(format!(
            "object {}: header says {} bytes but it holds {}",
            object.hash, declared, body.len()
        )));
        return damaged(kind);
    }

    let links = match kind {
        "commit" => parse_commit_object(&object.hash, body, issues),
        "tree" => parse_tree_object(&object.hash, body, issues),
        "tag" => parse_tag_object(&object.hash, body, issues),
        _ => Vec::new(),
    };
    FsckNode { kind: kind.to_string(), links }
}

/// Collects every ref file under `.git/refs` as `(ref name, content)`, skipping `.lock` files.
// Used by `check_refs` and `collect_labeled_roots`.
fn read_ref_files(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let name = format!("{}/{}", prefix, entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            read_ref_files(&entry.path(), &name, out)?;
        } else if !name.ends_with(".lock") {
            let content = fs::read_to_string(entry.path()).with_context(|| format!("Failed to read {}", name))?;
            out.push((name, content.trim().to_string()));
        }
    }
    Ok(())
}

/// Validates the refs and HEAD themselves: content must be a hash (or `ref: <target>`), and a
/// branch or a detached HEAD must point at a commit. (A ref pointing at a missing object is
/// reported by the connectivity walk, which knows how it is rooted.)
// Used by `run_fsck_checks`.
pub fn check_refs(graph: &HashMap<String, FsckNode>, issues: &mut Vec<FsckIssue>) -> Result<()> {
    let wants_commit = |name: &str| name == "HEAD" || name.starts_with("refs/heads/") || name.starts_with("refs/remotes/");

    let mut refs = Vec::new();
    if Path::new(".git/HEAD").is_file() {
        let head = fs::read_to_string(".git/HEAD").context("Failed to read .git/HEAD")?;
        refs.push(("HEAD".to_string(), head.trim().to_string()));
    }
    read_ref_files(Path::new(".git/refs"), "refs", &mut refs)?;

    for (name, content) in refs {
        if content.starts_with("ref: ") {
            continue;
        }
        if !is_object_hash(&content) {
            issues.push(FsckIssue::error(format!("{}: invalid content '{}'", name, content)));
            continue;
        }
        if let Some(node) = graph.get(&content) {
            if wants_commit(&name) && node.kind != "commit" && node.kind != "unknown" {
                issues.push(FsckIssue::error(format!("{}: points to a {}, not a commit ({})", name, node.kind, content)));
            }
        }
    }
    Ok(())
}

/// Validates the index: paths strictly ascending (so no duplicates), sane modes, and every entry
/// pointing at a blob. (An entry whose blob is missing is reported by the connectivity walk.)
// Used by `run_fsck_checks`.
pub fn check_index(graph: &HashMap<String, FsckNode>, issues: &mut Vec<FsckIssue>) {
    let entries = match read_index() {
        Ok(entries) => entries,
        Err(err) => {
            issues.push(FsckIssue::error(format!("index: unreadable: {}", err)));
            return;
        }
    };

    let mut previous: Option<&str> = None;
    for entry in &entries {
        match previous {
            Some(prev) if prev == entry.path => {
                issues.push(FsckIssue::error(format!("index: duplicate entry '{}'", entry.path)));
            }
            Some(prev) if prev > entry.path.as_str() => {
                issues.push(FsckIssue::warning(format!("index: entries not sorted ('{}' after '{}')", entry.path, prev)));
            }
            _ => {}
        }
        previous = Some(&entry.path);

        if entry.mode == 0o160000 {
            continue;
        }
        if !matches!(entry.mode, 0o100644 | 0o100755 | 0o120000) {
            issues.push(FsckIssue::warning(format!("index: entry '{}' has unusual mode {:o}", entry.path, entry.mode)));
        }
        if let Some(node) = graph.get(&hex::encode(entry.hash)) {
            if node.kind != "blob" && node.kind != "unknown" {
                issues.push(FsckIssue::error(format!("index: entry '{}' is a {}, not a blob", entry.path, node.kind)));
            }
        }
    }
}

/// Gathers everything that anchors objects as `(hash, label, strict)`: HEAD, refs, the index,
/// the stash and in-progress-operation files, and (optionally) every reflog entry. The label
/// says what to blame if the hash is missing; `strict` roots are errors when missing, the
/// others (history-only files and reflogs) only warnings.
// Used by `run_fsck_checks`.
pub fn collect_labeled_roots(include_reflogs: bool) -> Result<Vec<(String, String, bool)>> {
    let mut roots = Vec::new();

    for name in GC_STRICT_STATE_FILES {
        for hash in hashes_in_file(&Path::new(".git").join(name))? {
            roots.push((hash, name.to_string(), true));
        }
    }

    let mut ref_files = Vec::new();
    read_ref_files(Path::new(".git/refs"), "refs", &mut ref_files)?;
    for (name, content) in ref_files {
        if is_object_hash(&content) {
            roots.push((content, name, true));
        }
    }

    for entry in read_index().unwrap_or_default() {
        if entry.mode != 0o160000 {
            roots.push((hex::encode(entry.hash), format!("index entry '{}'", entry.path), true));
        }
    }

    for name in GC_LENIENT_STATE_FILES {
        for hash in hashes_in_file(&Path::new(".git").join(name))? {
            roots.push((hash, name.to_string(), false));
        }
    }

    if include_reflogs {
        for ref_name in list_reflog_refs()? {
            for (n, entry) in read_reflog(&ref_name)?.iter().enumerate() {
                for hash in [&entry.new_hash, &entry.old_hash] {
                    if hash != NULL_HASH {
                        roots.push((hash.clone(), format!("reflog {}@{{{}}}", ref_name, n), false));
                    }
                }
            }
        }
    }

    Ok(roots)
}

/// Walks the object graph from the roots, recording problems and returning what is reachable.
/// A missing root is reported against its label; a link from a reachable object to a missing
/// one is a `broken link` plus a `missing` report for the object; a link whose target is
/// the wrong type (a commit's "tree" that is a blob) is an error.
// Used by `run_fsck_checks`.
pub fn mark_reachable_in_graph(roots: &[(String, String, bool)], graph: &HashMap<String, FsckNode>, issues: &mut Vec<FsckIssue>) -> HashSet<String> {
    let mut reachable: HashSet<String> = HashSet::new();
    let mut stack: Vec<String> = Vec::new();
    let mut missing: BTreeSet<(String, String)> = BTreeSet::new();
    let mut reported_roots: HashSet<(String, String)> = HashSet::new();

    for (hash, label, strict) in roots {
        if graph.contains_key(hash) {
            stack.push(hash.clone());
        } else if reported_roots.insert((hash.clone(), label.clone())) {
            let message = format!("{}: points to missing object {}", label, hash);
            issues.push(if *strict { FsckIssue::error(message) } else { FsckIssue::warning(message) });
        }
    }

    while let Some(hash) = stack.pop() {
        if !reachable.insert(hash.clone()) {
            continue;
        }
        let Some(node) = graph.get(&hash) else { continue };

        for (child, expected) in &node.links {
            match graph.get(child) {
                None => {
                    issues.push(FsckIssue::error(format!("broken link from {} {} to {} {}", node.kind, hash, expected, child)));
                    missing.insert((child.clone(), expected.to_string()));
                }
                Some(target) => {
                    if *expected != "any" && target.kind != *expected && target.kind != "unknown" {
                        issues.push(FsckIssue::error(format!(
                            "{} {} links to {} as a {}, but it is a {}",
                            node.kind, hash, child, expected, target.kind
                        )));
                    }
                    stack.push(child.clone());
                }
            }
        }
    }

    for (hash, expected) in missing {
        issues.push(FsckIssue::error(format!("missing {} {}", expected, hash)));
    }
    reachable
}

/// Runs the whole check and returns the findings: verifies every loose object, builds the
/// object graph, validates refs, HEAD and the index, walks connectivity from every root, and
/// classifies what nothing reaches. An unreachable object that no other unreachable object
/// points at is `dangling`: the tip of an abandoned line of work, and what is worth recovering.
/// `connectivity_only` skips the per-object hash check, which is the slow part;
/// `include_reflogs` counts reflog entries as roots.
// Used by the `fsck` command.
pub fn run_fsck_checks(connectivity_only: bool, include_reflogs: bool) -> Result<FsckReport> {
    check_no_pack_files()?;

    let loose = list_loose_objects()?;
    let mut issues: Vec<FsckIssue> = Vec::new();
    let mut graph: HashMap<String, FsckNode> = HashMap::with_capacity(loose.len());
    for object in &loose {
        let node = verify_loose_object(object, !connectivity_only, &mut issues);
        graph.insert(object.hash.clone(), node);
    }

    check_refs(&graph, &mut issues)?;
    check_index(&graph, &mut issues);

    let roots = collect_labeled_roots(include_reflogs)?;
    let reachable = mark_reachable_in_graph(&roots, &graph, &mut issues);

    let mut unreachable: Vec<(String, String)> = graph
        .iter()
        .filter(|(hash, _)| !reachable.contains(*hash))
        .map(|(hash, node)| (node.kind.clone(), hash.clone()))
        .collect();
    unreachable.sort_by(|a, b| a.1.cmp(&b.1));

    let pointed_at: HashSet<&String> = unreachable
        .iter()
        .filter_map(|(_, hash)| graph.get(hash))
        .flat_map(|node| node.links.iter().map(|(child, _)| child))
        .collect();
    let dangling: Vec<(String, String)> = unreachable
        .iter()
        .filter(|(_, hash)| !pointed_at.contains(hash))
        .cloned()
        .collect();

    Ok(FsckReport { objects_checked: loose.len(), issues, dangling, unreachable })
}