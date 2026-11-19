use crate::diff::myers::myers_diff;
use crate::objects::DiffOp;
use crate::core::{commit::parse_commit_object, object_store::flatten_tree};
use crate::objects::{FileDiff, ParsedCommit};
use std::collections::BTreeMap;
use crate::objects::DiffStatEntry;
use crate::core::object_store::read_object;
use anyhow::Context;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;

const BASE85_ALPHABET: &[u8; 85] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz!#$%&()*+-;<=>?@^_`{|}~";

const CONTEXT_LINES: usize = 3;

struct Group {
    start: usize,
    end: usize,
}

pub const NO_EOL_MARK: &str = "\u{0}\\ No newline at end of file";

fn change_compact(rchg: &mut [bool], lines: &[String], rchgo: &mut [bool], indent_heuristic: bool) {
    let mut g = group_init(rchg);
    let mut go = group_init(rchgo);

    loop {
        if g.end != g.start {
            let mut earliest_end;
            let mut end_matching_other: i64;
            let mut groupsize;
            loop {
                groupsize = g.end - g.start;
                end_matching_other = -1;
                while group_slide_up(rchg, lines, &mut g) {
                    group_previous(rchgo, &mut go);
                }
                earliest_end = g.end;
                if go.end > go.start {
                    end_matching_other = g.end as i64;
                }
                while group_slide_down(rchg, lines, &mut g) {
                    group_next(rchgo, &mut go);
                    if go.end > go.start {
                        end_matching_other = g.end as i64;
                    }
                }
                if groupsize == g.end - g.start {
                    break;
                }
            }

            if g.end == earliest_end {
            } else if end_matching_other != -1 {
                while go.end == go.start {
                    group_slide_up(rchg, lines, &mut g);
                    group_previous(rchgo, &mut go);
                }
            } else if indent_heuristic {
                let mut shift = earliest_end as i64;
                if g.end as i64 - groupsize as i64 - 1 > shift {
                    shift = g.end as i64 - groupsize as i64 - 1;
                }
                if g.end as i64 - 100 > shift {
                    shift = g.end as i64 - 100;
                }
                let mut best_shift: i64 = -1;
                let mut best = (0i32, 0i32);
                while shift <= g.end as i64 {
                    let (mut indent, mut penalty) = (0, 0);
                    split_score(lines, shift as usize, &mut indent, &mut penalty);
                    split_score(lines, (shift - groupsize as i64) as usize, &mut indent, &mut penalty);
                    let cmp = 60 * ((indent > best.0) as i32 - (indent < best.0) as i32) + (penalty - best.1);
                    if best_shift == -1 || cmp <= 0 {
                        best = (indent, penalty);
                        best_shift = shift;
                    }
                    shift += 1;
                }
                while g.end as i64 > best_shift {
                    group_slide_up(rchg, lines, &mut g);
                    group_previous(rchgo, &mut go);
                }
            }
        }

        if !group_next(rchg, &mut g) {
            break;
        }
        group_next(rchgo, &mut go);
    }
}

fn changed(rchg: &[bool], i: i64) -> bool {
    i >= 0 && (i as usize) < rchg.len() && rchg[i as usize]
}

/// Diffs two line lists like xdiff: Myers diff, then change compaction (sliding each changed
/// group as far down as it goes, aligning with the other side, and with `indent_heuristic`
/// picking the most readable position). Returns the changed runs as
/// `(old_start, old_count, new_start, new_count)` plus the per-line change flags of each side.
// Used by `unified_hunks_with` and `merge-tree`'s line merge.
pub fn compacted_changes(old: &[String], new: &[String], indent_heuristic: bool) -> (Vec<(usize, usize, usize, usize)>, Vec<bool>, Vec<bool>) {
    let old_refs: Vec<&str> = old.iter().map(String::as_str).collect();
    let new_refs: Vec<&str> = new.iter().map(String::as_str).collect();
    let ops = myers_diff(&old_refs, &new_refs);

    let mut rchg_old = vec![false; old.len()];
    let mut rchg_new = vec![false; new.len()];
    let (mut i, mut j) = (0, 0);
    for op in &ops {
        match op {
            DiffOp::Keep(_) => {
                i += 1;
                j += 1;
            }
            DiffOp::Delete(_) => {
                rchg_old[i] = true;
                i += 1;
            }
            DiffOp::Insert(_) => {
                rchg_new[j] = true;
                j += 1;
            }
        }
    }
    change_compact(&mut rchg_old, old, &mut rchg_new, indent_heuristic);
    change_compact(&mut rchg_new, new, &mut rchg_old, indent_heuristic);

    let mut changes: Vec<(usize, usize, usize, usize)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if rchg_old.get(i).copied().unwrap_or(false) || rchg_new.get(j).copied().unwrap_or(false) {
            let (start_i, start_j) = (i, j);
            while rchg_old.get(i).copied().unwrap_or(false) {
                i += 1;
            }
            while rchg_new.get(j).copied().unwrap_or(false) {
                j += 1;
            }
            changes.push((start_i, i - start_i, start_j, j - start_j));
        } else {
            i += 1;
            j += 1;
        }
    }
    (changes, rchg_old, rchg_new)
}

fn decimal_width(n: usize) -> usize {
    n.to_string().len()
}

/// Produces git-format diffs for every path that differs between a commit and its first
/// parent (or the empty tree for a root commit), in path order.
// Used by `format-patch`.
pub fn diff_commit_against_parent(commit: &ParsedCommit) -> anyhow::Result<Vec<FileDiff>> {
    let mut old_tree = BTreeMap::new();
    if let Some(parent) = commit.parents.first() {
        let parent_commit = parse_commit_object(parent)?;
        flatten_tree(&parent_commit.tree, "", &mut old_tree)?;
    }
    let mut new_tree = BTreeMap::new();
    flatten_tree(&commit.tree, "", &mut new_tree)?;

    let paths: std::collections::BTreeSet<&String> = old_tree.keys().chain(new_tree.keys()).collect();
    let mut diffs = Vec::new();
    for path in paths {
        let old = old_tree.get(path).copied();
        let new = new_tree.get(path).copied();
        if old == new {
            continue;
        }
        match (old, new) {
            (Some(o), Some(n)) if (o.1 & 0o170000) != (n.1 & 0o170000) => {
                diffs.push(diff_tree_file(path, Some(o), None)?);
                diffs.push(diff_tree_file(path, None, Some(n))?);
            }
            _ => diffs.push(diff_tree_file(path, old, new)?),
        }
    }
    Ok(diffs)
}

/// `diff_tree_file` for contents that are already in memory, so a side can also be a file
/// from the working tree (its hash being the blob hash of that content).
// Used by `diff_tree_file` and the `diff-files` / `diff-index` / `diff-tree` commands.
pub fn diff_file_contents(path: &str, old: Option<([u8; 20], u32)>, new: Option<([u8; 20], u32)>, old_bytes: Vec<u8>, new_bytes: Vec<u8>) -> anyhow::Result<FileDiff> {
    let old_hex = old.map(|(h, _)| hex::encode(h)).unwrap_or_else(|| "0".repeat(40));
    let new_hex = new.map(|(h, _)| hex::encode(h)).unwrap_or_else(|| "0".repeat(40));

    let mut text = format!("diff --git a/{} b/{}\n", path, path);
    let mut summary = None;
    let mut index_mode_suffix = String::new();
    match (old, new) {
        (None, Some((_, mode))) => {
            text.push_str(&format!("new file mode {:06o}\n", mode));
            summary = Some(format!(" create mode {:06o} {}\n", mode, path));
        }
        (Some((_, mode)), None) => {
            text.push_str(&format!("deleted file mode {:06o}\n", mode));
            summary = Some(format!(" delete mode {:06o} {}\n", mode, path));
        }
        (Some((_, old_mode)), Some((_, new_mode))) if old_mode != new_mode => {
            text.push_str(&format!("old mode {:06o}\nnew mode {:06o}\n", old_mode, new_mode));
            summary = Some(format!(" mode change {:06o} => {:06o} {}\n", old_mode, new_mode, path));
        }
        (Some((_, mode)), Some(_)) => index_mode_suffix = format!(" {:06o}", mode),
        (None, None) => {}
    }

    let mut stat = DiffStatEntry { name: path.to_string(), ..Default::default() };
    if old_hex == new_hex {
        return Ok(FileDiff { text, stat, summary });
    }

    let is_binary = |b: &[u8]| b[..b.len().min(8000)].contains(&0);
    if is_binary(&old_bytes) || is_binary(&new_bytes) {
        text.push_str(&format!("index {}..{}{}\nGIT binary patch\n", old_hex, new_hex, index_mode_suffix));
        text.push_str(&encode_binary_literal(&new_bytes)?);
        text.push_str(&encode_binary_literal(&old_bytes)?);
        stat.binary_sizes = Some((old_bytes.len(), new_bytes.len()));
        return Ok(FileDiff { text, stat, summary });
    }

    text.push_str(&format!("index {}..{}{}\n", &old_hex[..7], &new_hex[..7], index_mode_suffix));
    if old_bytes.is_empty() && new_bytes.is_empty() {
        return Ok(FileDiff { text, stat, summary });
    }
    text.push_str(&format!("--- {}\n", if old.is_some() { format!("a/{}", path) } else { "/dev/null".to_string() }));
    text.push_str(&format!("+++ {}\n", if new.is_some() { format!("b/{}", path) } else { "/dev/null".to_string() }));

    let old_lines = split_diff_lines(&old_bytes);
    let new_lines = split_diff_lines(&new_bytes);
    let (hunks, added, deleted) = unified_hunks(&old_lines, &new_lines);
    text.push_str(&hunks);
    stat.added = added;
    stat.deleted = deleted;
    Ok(FileDiff { text, stat, summary })
}

/// Produces the `diff --git` section for one path: mode/creation/deletion headers, the
/// `index` line, and either unified hunks (3 lines of context, function-name headers, git's
/// hunk placement) or a `GIT binary patch` with zlib + base85 literals in both directions.
// Used by `format-patch` via `diff_commit_against_parent`.
pub fn diff_tree_file(path: &str, old: Option<([u8; 20], u32)>, new: Option<([u8; 20], u32)>) -> anyhow::Result<FileDiff> {
    let load = |entry: Option<([u8; 20], u32)>| -> anyhow::Result<Vec<u8>> {
        match entry {
            // A submodule entry names a commit of another repository; like git, show it as
            // a one-line pseudo file.
            Some((hash, mode)) if mode == 0o160000 => Ok(format!("Subproject commit {}\n", hex::encode(hash)).into_bytes()),
            Some((hash, _)) => Ok(read_object(&hex::encode(hash))?.1),
            None => Ok(Vec::new()),
        }
    };
    diff_file_contents(path, old, new, load(old)?, load(new)?)
}

fn display_line(line: &str) -> String {
    match line.strip_suffix(NO_EOL_MARK) {
        Some(text) => format!("{}\n\\ No newline at end of file\n", text),
        None => format!("{}\n", line),
    }
}

/// Encodes one direction of a `GIT binary patch`: `literal <size>` followed by the zlib-deflated
/// data in base85 lines of up to 52 bytes (prefixed with a length letter), and a blank line.
// Used by `format-patch` via `diff_tree_file`.
pub fn encode_binary_literal(data: &[u8]) -> anyhow::Result<String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(data).context("Failed to deflate binary data")?;
    let deflated = encoder.finish()?;

    let mut out = format!("literal {}\n", data.len());
    for chunk in deflated.chunks(52) {
        let len_char = if chunk.len() <= 26 { b'A' + chunk.len() as u8 - 1 } else { b'a' + chunk.len() as u8 - 27 };
        out.push(len_char as char);
        for group in chunk.chunks(4) {
            let mut word = [0u8; 4];
            word[..group.len()].copy_from_slice(group);
            let mut value = u32::from_be_bytes(word);
            let mut encoded = [0u8; 5];
            for slot in encoded.iter_mut().rev() {
                *slot = BASE85_ALPHABET[(value % 85) as usize];
                value /= 85;
            }
            out.push_str(std::str::from_utf8(&encoded).unwrap_or_default());
        }
        out.push('\n');
    }
    out.push('\n');
    Ok(out)
}

/// Formats a diffstat block exactly like git's `--stat` for mail (72 columns): name column,
/// change count, scaled `+`/`-` graph (`Bin X -> Y bytes` for binaries), and the
/// `N files changed, X insertions(+), Y deletions(-)` summary.
// Used by `format-patch`.
pub fn format_diffstat(entries: &[DiffStatEntry], width: usize) -> String {
    let mut max_len = 0;
    let mut max_change = 0;
    let mut number_width = 0;
    let mut bin_width = 0;
    for e in entries {
        max_len = max_len.max(e.name.chars().count());
        if e.unmerged {
            // "Unmerged" is 8 characters.
            bin_width = bin_width.max(8);
            continue;
        }
        if let Some((old, new)) = e.binary_sizes {
            bin_width = bin_width.max(14 + decimal_width(new) + decimal_width(old));
            number_width = 3;
            continue;
        }
        max_change = max_change.max(e.added + e.deleted);
    }
    number_width = number_width.max(decimal_width(max_change));
    let width = width.max(16 + 6 + number_width) as i64;
    let number_width_i = number_width as i64;

    let mut graph_width = if max_change + 4 > bin_width { max_change as i64 } else { bin_width as i64 - 4 };
    let mut name_width = max_len as i64;
    if name_width + number_width_i + 6 + graph_width > width {
        if graph_width > width * 3 / 8 - number_width_i - 6 {
            graph_width = (width * 3 / 8 - number_width_i - 6).max(6);
        }
        if name_width > width - number_width_i - 6 - graph_width {
            name_width = width - number_width_i - 6 - graph_width;
        } else {
            graph_width = width - number_width_i - 6 - name_width;
        }
    }

    let mut out = String::new();
    let (mut adds, mut dels) = (0, 0);
    for e in entries {
        let mut name: &str = &e.name;
        let mut prefix = "";
        let mut len = name_width;
        let name_len = name.chars().count() as i64;
        if name_width < name_len {
            prefix = "...";
            len = (len - 3).max(0);
            let skip = (name_len - len).max(0) as usize;
            let byte_index = name.char_indices().nth(skip).map(|(b, _)| b).unwrap_or(name.len());
            name = &name[byte_index..];
            if let Some(slash) = name.find('/') {
                name = &name[slash..];
            }
        }
        let padding = (len - name.chars().count() as i64).max(0) as usize;

        if e.unmerged {
            out.push_str(&format!(" {}{}{} | {:>w$}\n", prefix, name, " ".repeat(padding), "Unmerged", w = number_width));
            continue;
        }
        if let Some((old, new)) = e.binary_sizes {
            out.push_str(&format!(" {}{}{} | {:>w$}", prefix, name, " ".repeat(padding), "Bin", w = number_width));
            if old != 0 || new != 0 {
                out.push_str(&format!(" {} -> {} bytes", old, new));
            }
            out.push('\n');
            continue;
        }

        adds += e.added;
        dels += e.deleted;
        let (mut add, mut del) = (e.added as i64, e.deleted as i64);
        if graph_width <= max_change as i64 {
            let scale = |it: i64| if it == 0 { 0 } else { 1 + it * (graph_width - 1) / max_change as i64 };
            let mut total = scale(add + del);
            if total < 2 && add > 0 && del > 0 {
                total = 2;
            }
            if add < del {
                add = scale(add);
                del = total - add;
            } else {
                del = scale(del);
                add = total - del;
            }
        }
        let count = e.added + e.deleted;
        out.push_str(&format!(
            " {}{}{} | {:>w$}{}{}{}\n",
            prefix,
            name,
            " ".repeat(padding),
            count,
            if count > 0 { " " } else { "" },
            "+".repeat(add.max(0) as usize),
            "-".repeat(del.max(0) as usize),
            w = number_width
        ));
    }

    let files = entries.iter().filter(|e| !e.unmerged).count();
    if files == 0 {
        out.push_str(" 0 files changed\n");
        return out;
    }
    let mut summary = format!(" {} file{} changed", files, if files == 1 { "" } else { "s" });
    if adds > 0 || dels == 0 {
        summary.push_str(&format!(", {} insertion{}(+)", adds, if adds == 1 { "" } else { "s" }));
    }
    if dels > 0 || adds == 0 {
        summary.push_str(&format!(", {} deletion{}(-)", dels, if dels == 1 { "" } else { "s" }));
    }
    out.push_str(&summary);
    out.push('\n');
    out
}

fn get_indent(line: &str) -> i32 {
    let mut ret = 0;
    for c in line.strip_suffix(NO_EOL_MARK).unwrap_or(line).bytes() {
        if !matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            return ret;
        } else if c == b' ' {
            ret += 1;
        } else if c == b'\t' {
            ret += 8 - ret % 8;
        }
        if ret >= 200 {
            return 200;
        }
    }
    -1
}

fn group_init(rchg: &[bool]) -> Group {
    let mut end = 0;
    while changed(rchg, end as i64) {
        end += 1;
    }
    Group { start: 0, end }
}

fn group_next(rchg: &[bool], g: &mut Group) -> bool {
    if g.end == rchg.len() {
        return false;
    }
    g.start = g.end + 1;
    g.end = g.start;
    while changed(rchg, g.end as i64) {
        g.end += 1;
    }
    true
}

fn group_previous(rchg: &[bool], g: &mut Group) -> bool {
    if g.start == 0 {
        return false;
    }
    g.end = g.start - 1;
    g.start = g.end;
    while changed(rchg, g.start as i64 - 1) {
        g.start -= 1;
    }
    true
}

fn group_slide_down(rchg: &mut [bool], lines: &[String], g: &mut Group) -> bool {
    if g.end < lines.len() && lines[g.start] == lines[g.end] {
        rchg[g.start] = false;
        rchg[g.end] = true;
        g.start += 1;
        g.end += 1;
        while changed(rchg, g.end as i64) {
            g.end += 1;
        }
        return true;
    }
    false
}

fn group_slide_up(rchg: &mut [bool], lines: &[String], g: &mut Group) -> bool {
    if g.start > 0 && lines[g.start - 1] == lines[g.end - 1] {
        g.start -= 1;
        g.end -= 1;
        rchg[g.start] = true;
        rchg[g.end] = false;
        while changed(rchg, g.start as i64 - 1) {
            g.start -= 1;
        }
        return true;
    }
    false
}

fn match_func_line(line: &str) -> Option<String> {
    let text = line.strip_suffix(NO_EOL_MARK).unwrap_or(line);
    let first = text.bytes().next()?;
    if !(first.is_ascii_alphabetic() || first == b'_' || first == b'$') {
        return None;
    }
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(b'\n');
    bytes.truncate(80);
    while bytes.last().is_some_and(|b| b.is_ascii_whitespace()) {
        bytes.pop();
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Splits blob bytes into diff lines, marking a missing final newline the way the hunk
/// formatter expects.
// Used by `format-patch` via `diff_tree_file`, and `cherry` patch IDs.
pub fn split_diff_lines(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    if text.ends_with('\n') {
        lines.pop();
    } else if let Some(last) = lines.last_mut() {
        last.push_str(NO_EOL_MARK);
    }
    lines
}

fn split_score(lines: &[String], split: usize, effective_indent: &mut i32, penalty: &mut i32) {
    let end_of_file = split >= lines.len();
    let indent = if end_of_file { -1 } else { get_indent(&lines[split]) };

    let mut pre_blank = 0;
    let mut pre_indent = -1;
    let mut k = split as i64 - 1;
    while k >= 0 {
        pre_indent = get_indent(&lines[k as usize]);
        if pre_indent != -1 {
            break;
        }
        pre_blank += 1;
        if pre_blank == 20 {
            pre_indent = 0;
            break;
        }
        k -= 1;
    }

    let mut post_blank = 0;
    let mut post_indent = -1;
    for line in lines.iter().skip(split + 1) {
        post_indent = get_indent(line);
        if post_indent != -1 {
            break;
        }
        post_blank += 1;
        if post_blank == 20 {
            post_indent = 0;
            break;
        }
    }

    if pre_indent == -1 && pre_blank == 0 {
        *penalty += 1;
    }
    if end_of_file {
        *penalty += 21;
    }
    let post_blank_total = if indent == -1 { 1 + post_blank } else { 0 };
    let total_blank = pre_blank + post_blank_total;
    *penalty += -30 * total_blank;
    *penalty += 6 * post_blank_total;
    let indent = if indent != -1 { indent } else { post_indent };
    let any_blanks = total_blank != 0;
    *effective_indent += indent;

    if indent == -1 || pre_indent == -1 || indent == pre_indent {
    } else if indent > pre_indent {
        *penalty += if any_blanks { 10 } else { -4 };
    } else if post_indent != -1 && post_indent > indent {
        *penalty += if any_blanks { 17 } else { 24 };
    } else {
        *penalty += if any_blanks { 17 } else { 23 };
    }
}

/// Computes git-style unified hunks between two line lists: Myers diff, then xdiff's change
/// compaction (sliding groups, aligning with the other side, indent heuristic), then hunks
/// with 3 lines of context merged when at most 6 lines apart, each headed by the nearest
/// preceding "function" line (one starting with a letter, `_`, or `$`). Returns the hunk text
/// and the added/deleted line counts.
// Used by `format-patch` via `diff_tree_file`.
pub fn unified_hunks(old: &[String], new: &[String]) -> (String, usize, usize) {
    unified_hunks_with(old, new, true)
}

/// Same as `unified_hunks`, but the indent heuristic can be turned off: without it, xdiff
/// leaves each sliding group at its lowest position (as git does when computing patch IDs).
// Used by `unified_hunks` and `cherry` patch IDs.
pub fn unified_hunks_with(old: &[String], new: &[String], indent_heuristic: bool) -> (String, usize, usize) {
    let (changes, rchg_old, rchg_new) = compacted_changes(old, new, indent_heuristic);

    let added = rchg_new.iter().filter(|&&c| c).count();
    let deleted = rchg_old.iter().filter(|&&c| c).count();
    let mut out = String::new();
    let mut func_line = String::new();
    let mut func_limit: i64 = -1;
    let mut c = 0;

    while c < changes.len() {
        let mut last = c;
        while last + 1 < changes.len() {
            let (pi, pc, _, _) = changes[last];
            let (ni, _, _, _) = changes[last + 1];
            if ni - (pi + pc) > 2 * CONTEXT_LINES {
                break;
            }
            last += 1;
        }
        let (first_i, _, first_j, _) = changes[c];
        let (last_i, last_ci, last_j, last_cj) = changes[last];
        let s1 = first_i.saturating_sub(CONTEXT_LINES);
        let s2 = first_j.saturating_sub(CONTEXT_LINES);
        let trailing = CONTEXT_LINES.min(old.len() - (last_i + last_ci)).min(new.len() - (last_j + last_cj));
        let e1 = last_i + last_ci + trailing;
        let e2 = last_j + last_cj + trailing;

        let start = s1 as i64 - 1;
        let mut l = start;
        while l != func_limit && l >= 0 && (l as usize) < old.len() {
            if let Some(found) = match_func_line(&old[l as usize]) {
                func_line = found;
                break;
            }
            l -= 1;
        }
        func_limit = start;

        let range = |s: usize, n: usize| -> String {
            let first = if n > 0 { s + 1 } else { s };
            if n == 1 { format!("{}", first) } else { format!("{},{}", first, n) }
        };
        out.push_str(&format!("@@ -{} +{} @@", range(s1, e1 - s1), range(s2, e2 - s2)));
        if !func_line.is_empty() {
            out.push(' ');
            out.push_str(&func_line);
        }
        out.push('\n');

        let (mut i, mut j) = (s1, s2);
        while i < e1 || j < e2 {
            let old_changed = i < e1 && rchg_old[i];
            let new_changed = j < e2 && rchg_new[j];
            if old_changed {
                out.push('-');
                out.push_str(&display_line(&old[i]));
                i += 1;
            } else if new_changed {
                out.push('+');
                out.push_str(&display_line(&new[j]));
                j += 1;
            } else {
                out.push(' ');
                out.push_str(&display_line(&new[j]));
                i += 1;
                j += 1;
            }
        }
        c = last + 1;
    }

    (out, added, deleted)
}
