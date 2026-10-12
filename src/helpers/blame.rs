use anyhow::{Context, Result};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::helpers::github::format_unix_timestamp_iso8601;
use crate::helpers::{commit_parents, myers_diff, read_object, tree_hash_of_commit};
use crate::objects::{BlameLine, DiffOp};

/// The commit hash recorded for lines that exist only in the working copy.
const UNCOMMITTED_HASH: &str = "0000000000000000000000000000000000000000";

/// Parses the `-L` argument of `blame` (`START,END`, `START,+COUNT`, or `START,` / `START`
/// for "to the end of the file") into an inclusive, 1-based `(start, end)` range, clamping
/// `end` to the file's length.
// Used by `blame`.
pub fn parse_line_range(spec: &str, total: usize) -> Result<(usize, usize)> {
    let invalid = || {
        anyhow::anyhow!(
            "fatal: invalid -L argument '{}' (expected START,END, START,+COUNT or START,)",
            spec
        )
    };

    let (start_part, end_part) = spec.split_once(',').unwrap_or((spec, ""));
    let start: usize = start_part.trim().parse().map_err(|_| invalid())?;
    let end_part = end_part.trim();

    let end: usize = if end_part.is_empty() {
        total
    } else if let Some(count) = end_part.strip_prefix('+') {
        let count: usize = count.parse().map_err(|_| invalid())?;
        if count == 0 {
            return Err(invalid());
        }
        start + count - 1
    } else {
        end_part.parse().map_err(|_| invalid())?
    };

    if start == 0 || end < start {
        return Err(invalid());
    }
    if start > total {
        anyhow::bail!("fatal: file has only {} line{}", total, if total == 1 { "" } else { "s" });
    }

    Ok((start, end.min(total)))
}

/// Looks one name up in the raw content of a tree object, returning its `(mode, hex hash)`.
// Used by `blame`.
fn find_tree_entry(content: &[u8], name: &str) -> Result<Option<(String, String)>> {
    let mut pos = 0;
    while pos < content.len() {
        let space = content[pos..]
            .iter()
            .position(|&b| b == b' ')
            .context("Missing mode separator in tree object")?
            + pos;
        let nul = content[space..]
            .iter()
            .position(|&b| b == 0)
            .context("Missing name terminator in tree object")?
            + space;
        let hash_end = nul + 21;
        if hash_end > content.len() {
            anyhow::bail!("Corrupted tree object: incomplete hash");
        }

        if &content[space + 1..nul] == name.as_bytes() {
            let mode = String::from_utf8_lossy(&content[pos..space]).to_string();
            return Ok(Some((mode, hex::encode(&content[nul + 1..hash_end]))));
        }
        pos = hash_end;
    }
    Ok(None)
}

/// Finds the blob hash of the file at `path` in a commit's tree, reading only the tree objects
/// along the path rather than flattening the whole tree. Returns `None` if the path does not
/// exist in that commit or names a directory.
// Used by `blame`.
pub fn blob_hash_at_path(commit_hash: &str, path: &str) -> Result<Option<String>> {
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    if components.is_empty() {
        return Ok(None);
    }

    let mut tree_hash = tree_hash_of_commit(commit_hash)?;
    for (i, component) in components.iter().enumerate() {
        let is_last = i + 1 == components.len();
        let (object_type, content) = read_object(&tree_hash)?;
        if object_type != "tree" {
            anyhow::bail!("{} is not a tree object", tree_hash);
        }

        let Some((mode, hash)) = find_tree_entry(&content, component)? else {
            return Ok(None);
        };
        let is_dir = mode == "40000" || mode == "040000";

        if is_last {
            return Ok(if is_dir { None } else { Some(hash) });
        }
        if !is_dir {
            return Ok(None);
        }
        tree_hash = hash;
    }

    Ok(None)
}

/// Same as `blob_hash_at_path`, but remembers answers per commit, since the walk asks about
/// every commit twice (once as a child, once as a parent).
// Used by `blame`.
fn cached_blob_hash(cache: &mut HashMap<String, Option<String>>, commit_hash: &str, path: &str) -> Result<Option<String>> {
    if let Some(known) = cache.get(commit_hash) {
        return Ok(known.clone());
    }
    let found = blob_hash_at_path(commit_hash, path)?;
    cache.insert(commit_hash.to_string(), found.clone());
    Ok(found)
}

/// Splits file bytes into lines, refusing binary content (anything containing a NUL byte).
// Used by `blame`.
fn split_text_lines(bytes: &[u8], path: &str) -> Result<Vec<String>> {
    if bytes.contains(&0) {
        anyhow::bail!("fatal: cannot blame binary file '{}'", path);
    }
    Ok(String::from_utf8_lossy(bytes).lines().map(|l| l.to_string()).collect())
}

/// Reads a blob's lines, remembering them per blob hash so repeated visits are free.
// Used by `blame`.
fn cached_blob_lines(cache: &mut HashMap<String, Rc<Vec<String>>>, blob_hash: &str, path: &str) -> Result<Rc<Vec<String>>> {
    if let Some(known) = cache.get(blob_hash) {
        return Ok(Rc::clone(known));
    }
    let (object_type, content) = read_object(blob_hash)?;
    if object_type != "blob" {
        anyhow::bail!("{} is not a blob object", blob_hash);
    }
    let lines = Rc::new(split_text_lines(&content, path)?);
    cache.insert(blob_hash.to_string(), Rc::clone(&lines));
    Ok(lines)
}

/// Diffs two versions of a file and maps every line of `new_lines` that survived unchanged
/// to its index in `old_lines`. Lines that were inserted or rewritten have no entry.
// Used by `blame`.
fn keep_map(old_lines: &[String], new_lines: &[String]) -> HashMap<usize, usize> {
    let old_refs: Vec<&str> = old_lines.iter().map(|l| l.as_str()).collect();
    let new_refs: Vec<&str> = new_lines.iter().map(|l| l.as_str()).collect();

    let mut kept = HashMap::new();
    let (mut old_idx, mut new_idx) = (0usize, 0usize);
    for op in myers_diff(&old_refs, &new_refs) {
        match op {
            DiffOp::Keep(_) => {
                kept.insert(new_idx, old_idx);
                old_idx += 1;
                new_idx += 1;
            }
            DiffOp::Delete(_) => old_idx += 1,
            DiffOp::Insert(_) => new_idx += 1,
        }
    }
    kept
}

/// Parses a commit's `author` header into `(name, unix timestamp, timezone offset)`.
// Used by `blame`.
fn parse_author_line(line: &str) -> Option<(String, u64, String)> {
    let content = line.strip_prefix("author ")?;
    let open = content.find('<')?;
    let close = content.find('>')?;
    if close < open {
        return None;
    }

    let name = content[..open].trim().to_string();
    let mut rest = content[close + 1..].split_whitespace();
    let timestamp: u64 = rest.next()?.parse().ok()?;
    let tz = rest
        .next()
        .filter(|t| t.len() == 5 && t.starts_with(['+', '-']) && t[1..].chars().all(|c| c.is_ascii_digit()))
        .unwrap_or("+0000")
        .to_string();

    Some((name, timestamp, tz))
}

/// Reads who wrote a commit and when, plus whether it is a root commit (which `blame` marks
/// as a boundary with a leading `^`).
// Used by `blame`.
fn commit_author_info(commit_hash: &str) -> Result<(String, u64, String, bool)> {
    let (object_type, content) = read_object(commit_hash)?;
    if object_type != "commit" {
        anyhow::bail!("{} is not a commit object", commit_hash);
    }
    let text = String::from_utf8_lossy(&content);

    let (author, timestamp, tz) = text
        .lines()
        .take_while(|l| !l.is_empty())
        .find_map(parse_author_line)
        .unwrap_or_else(|| ("unknown".to_string(), 0, "+0000".to_string()));

    Ok((author, timestamp, tz, commit_parents(&text).is_empty()))
}

/// Attributes each line of `path` to the commit that last changed it.
///
/// Every requested line is carried back through history as a `(final line, line in the
/// commit being examined)` pair. At each commit the file is diffed against each parent in
/// turn: a line that survives unchanged is handed to that parent, and a line that no parent
/// has is blamed on the commit itself. With `use_working_copy`, the file on disk is first
/// diffed against `commit_hash`'s version and lines it changed are marked as not committed
/// yet. `line_spec` is an optional `-L` range. File renames are not followed.
// Used by `blame`.
pub fn blame_file(path: &str, commit_hash: &str, use_working_copy: bool, line_spec: Option<&str>) -> Result<Vec<BlameLine>> {
    let mut blob_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut lines_cache: HashMap<String, Rc<Vec<String>>> = HashMap::new();

    let tip_blob = cached_blob_hash(&mut blob_cache, commit_hash, path)?
        .ok_or_else(|| anyhow::anyhow!("fatal: no such path '{}' in {}", path, &commit_hash[..7]))?;
    let tip_lines = cached_blob_lines(&mut lines_cache, &tip_blob, path)?;

    let working_lines: Option<Vec<String>> = if use_working_copy {
        match fs::read(path) {
            Ok(bytes) => Some(split_text_lines(&bytes, path)?),
            Err(_) => None,
        }
    } else {
        None
    };
    let final_lines: Vec<String> = match &working_lines {
        Some(working) => working.clone(),
        None => tip_lines.as_ref().clone(),
    };

    let total = final_lines.len();
    if total == 0 {
        return Ok(Vec::new());
    }
    let (start, end) = match line_spec {
        Some(spec) => parse_line_range(spec, total)?,
        None => (1, total),
    };

    let mut blamed: Vec<Option<String>> = vec![None; total];
    let mut mapping: Vec<(usize, usize)> = (start - 1..end).map(|i| (i, i)).collect();

    if let Some(working) = &working_lines {
        if *working != *tip_lines {
            let kept = keep_map(&tip_lines, working);
            let mut passed = Vec::new();
            for (final_idx, idx) in mapping.drain(..) {
                match kept.get(&idx) {
                    Some(&old_idx) => passed.push((final_idx, old_idx)),
                    None => blamed[final_idx] = Some(UNCOMMITTED_HASH.to_string()),
                }
            }
            mapping = passed;
        }
    }

    let mut queue: VecDeque<(String, Vec<(usize, usize)>)> = VecDeque::new();
    queue.push_back((commit_hash.to_string(), mapping));

    while let Some((commit, mapping)) = queue.pop_front() {
        if mapping.is_empty() {
            continue;
        }

        let Some(current_blob) = cached_blob_hash(&mut blob_cache, &commit, path)? else {
            for (final_idx, _) in &mapping {
                blamed[*final_idx] = Some(commit.clone());
            }
            continue;
        };
        let current_lines = cached_blob_lines(&mut lines_cache, &current_blob, path)?;

        let (_, commit_content) = read_object(&commit)?;
        let parents = commit_parents(&String::from_utf8_lossy(&commit_content));

        let mut remaining = mapping;
        for parent in &parents {
            if remaining.is_empty() {
                break;
            }
            let Some(parent_blob) = cached_blob_hash(&mut blob_cache, parent, path)? else {
                continue;
            };

            let mut passed: Vec<(usize, usize)> = Vec::new();
            if parent_blob == current_blob {
                passed = std::mem::take(&mut remaining);
            } else {
                let parent_lines = cached_blob_lines(&mut lines_cache, &parent_blob, path)?;
                let kept = keep_map(&parent_lines, &current_lines);
                let mut rest = Vec::new();
                for (final_idx, idx) in remaining.drain(..) {
                    match kept.get(&idx) {
                        Some(&old_idx) => passed.push((final_idx, old_idx)),
                        None => rest.push((final_idx, idx)),
                    }
                }
                remaining = rest;
            }

            if !passed.is_empty() {
                queue.push_back((parent.clone(), passed));
            }
        }

        for (final_idx, _) in remaining {
            blamed[final_idx] = Some(commit.clone());
        }
    }

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let mut author_cache: HashMap<String, (String, u64, String, bool)> = HashMap::new();
    let mut result = Vec::with_capacity(end - start + 1);

    for idx in start - 1..end {
        let commit = blamed[idx].clone().ok_or_else(|| {
            anyhow::anyhow!("internal error: line {} of '{}' was never attributed to a commit", idx + 1, path)
        })?;

        let (author, timestamp, tz, boundary) = if commit == UNCOMMITTED_HASH {
            ("Not Committed Yet".to_string(), now, "+0000".to_string(), false)
        } else {
            if !author_cache.contains_key(&commit) {
                author_cache.insert(commit.clone(), commit_author_info(&commit)?);
            }
            author_cache[&commit].clone()
        };

        result.push(BlameLine {
            commit,
            author,
            timestamp,
            tz,
            line_no: idx + 1,
            content: final_lines[idx].clone(),
            boundary,
        });
    }

    Ok(result)
}

/// Turns a `+HHMM` / `-HHMM` offset into seconds east of UTC (0 if it doesn't parse).
// Used by `blame`.
fn parse_tz_offset(tz: &str) -> i64 {
    if tz.len() != 5 {
        return 0;
    }
    let sign = match tz.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return 0,
    };
    let hours: i64 = tz[1..3].parse().unwrap_or(0);
    let minutes: i64 = tz[3..5].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

/// Formats a commit timestamp as the author's local time, `YYYY-MM-DD HH:MM:SS +ZZZZ`.
// Used by `blame`.
fn format_author_time(timestamp: u64, tz: &str) -> String {
    let local = (timestamp as i64 + parse_tz_offset(tz)).max(0) as u64;
    let iso = format_unix_timestamp_iso8601(local);
    format!("{} {} {}", &iso[..10], &iso[11..19], tz)
}

/// Renders blame results as one line per source line: abbreviated hash, author, author time,
/// line number and the line itself, with the author and line-number columns padded to align.
// Used by `blame`.
pub fn format_blame_output(lines: &[BlameLine]) -> String {
    let author_width = lines.iter().map(|l| l.author.chars().count()).max().unwrap_or(0);
    let number_width = lines.iter().map(|l| l.line_no).max().unwrap_or(0).to_string().len();

    let mut out = String::new();
    for line in lines {
        out.push_str(&format!(
            "{} ({:<aw$} {} {:>nw$}) {}\n",
            line.hash_label(),
            line.author,
            format_author_time(line.timestamp, &line.tz),
            line.line_no,
            line.content,
            aw = author_width,
            nw = number_width,
        ));
    }
    out
}