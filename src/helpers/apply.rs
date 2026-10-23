use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Read;
use std::path::Path;
use anyhow::Context;
use crate::helpers::{hash_content, read_object};
use crate::index::IndexEntry;
use crate::objects::{ApplyOptions, FilePatch, HunkFailure, HunkLine, HunkLineKind, PatchHunk, PatchImageLine};

/// Reads the patch text from the given files, concatenated in order, or from standard input
/// when no file (or `-`) is given.
// Used by `apply`.
pub fn read_patch_input(paths: &[String]) -> anyhow::Result<String> {
    let mut bytes = Vec::new();
    if paths.is_empty() {
        std::io::stdin().read_to_end(&mut bytes).context("Failed to read patch from standard input")?;
    } else {
        for path in paths {
            if path == "-" {
                std::io::stdin().read_to_end(&mut bytes).context("Failed to read patch from standard input")?;
            } else {
                let content = fs::read(path).map_err(|_| anyhow::anyhow!("error: can't open patch '{}': No such file or directory", path))?;
                bytes.extend_from_slice(&content);
            }
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Parses git-style (`diff --git`) and traditional unified (`---`/`+++`) diffs into per-file
/// patches. Text outside of patches (mail headers, commit messages, signatures) is skipped,
/// so `format-patch` output works. `strip` removes leading path components like `-p<n>`.
/// Fails with git's `corrupt patch at line N` when a hunk's line counts don't add up.
// Used by `apply`.
pub fn parse_patch(text: &str, strip: usize) -> anyhow::Result<Vec<FilePatch>> {
    let lines: Vec<&str> = text.strip_suffix('\n').unwrap_or(text).split('\n').collect();
    let lines: Vec<&str> = if text.is_empty() { Vec::new() } else { lines };
    let mut patches = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = trim_cr(lines[i]);
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let (patch, next) = parse_git_patch(&lines, i, rest, strip)?;
            patches.push(patch);
            i = next;
        } else if line.starts_with("--- ") && i + 2 < lines.len() && trim_cr(lines[i + 1]).starts_with("+++ ") && trim_cr(lines[i + 2]).starts_with("@@ ") {
            let old = traditional_name(&line[4..], strip);
            let new = traditional_name(&trim_cr(lines[i + 1])[4..], strip);
            let mut patch = FilePatch {
                is_new: old.is_none(),
                is_delete: new.is_none(),
                ..Default::default()
            };
            match (&old, &new) {
                (Some(_), Some(n)) => {
                    patch.old_path = Some(n.clone());
                    patch.new_path = Some(n.clone());
                }
                _ => {
                    patch.old_path = old;
                    patch.new_path = new;
                }
            }
            let (hunks, next) = parse_hunks(&lines, i + 2)?;
            patch.hunks = hunks;
            patches.push(patch);
            i = next;
        } else {
            i += 1;
        }
    }

    Ok(patches)
}

fn parse_git_patch(lines: &[&str], start: usize, header_rest: &str, strip: usize) -> anyhow::Result<(FilePatch, usize)> {
    let (header_a, header_b) = split_git_header_names(header_rest);
    let mut patch = FilePatch::default();
    let mut strip_failed = false;
    let mut saw_minus: Option<Option<String>> = None;
    let mut saw_plus: Option<Option<String>> = None;
    let mut rename_from = None;
    let mut rename_to = None;
    let mut i = start + 1;

    while i < lines.len() {
        let line = trim_cr(lines[i]);
        if line.starts_with("diff --git ") || line.starts_with("@@ ") {
            break;
        } else if let Some(m) = line.strip_prefix("old mode ") {
            patch.old_mode = parse_mode(m);
        } else if let Some(m) = line.strip_prefix("new mode ") {
            patch.new_mode = parse_mode(m);
        } else if let Some(m) = line.strip_prefix("deleted file mode ") {
            patch.is_delete = true;
            patch.old_mode = parse_mode(m);
        } else if let Some(m) = line.strip_prefix("new file mode ") {
            patch.is_new = true;
            patch.new_mode = parse_mode(m);
        } else if let Some(p) = line.strip_prefix("rename from ") {
            patch.is_rename = true;
            rename_from = Some(unquote_path(p));
        } else if let Some(p) = line.strip_prefix("rename to ") {
            patch.is_rename = true;
            rename_to = Some(unquote_path(p));
        } else if let Some(p) = line.strip_prefix("copy from ") {
            patch.is_copy = true;
            rename_from = Some(unquote_path(p));
        } else if let Some(p) = line.strip_prefix("copy to ") {
            patch.is_copy = true;
            rename_to = Some(unquote_path(p));
        } else if let Some(s) = line.strip_prefix("similarity index ") {
            patch.similarity = s.trim_end_matches('%').parse().ok();
        } else if line.starts_with("dissimilarity index ") {
        } else if let Some(idx) = line.strip_prefix("index ") {
            if let Some(mode) = idx.split_whitespace().nth(1) {
                patch.old_mode = patch.old_mode.or(parse_mode(mode));
                patch.new_mode = patch.new_mode.or(parse_mode(mode));
            }
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            patch.is_binary = true;
            i += 1;
            while i < lines.len() && !trim_cr(lines[i]).starts_with("diff --git ") {
                i += 1;
            }
            break;
        } else if let Some(name) = line.strip_prefix("--- ") {
            let (side, ok) = git_side_name_checked(name, strip);
            strip_failed |= !ok;
            saw_minus = Some(side);
        } else if let Some(name) = line.strip_prefix("+++ ") {
            let (side, ok) = git_side_name_checked(name, strip);
            strip_failed |= !ok;
            saw_plus = Some(side);
        } else {
            break;
        }
        i += 1;
    }

    let names_given = rename_from.is_some() && rename_to.is_some();
    let header_strippable = [&header_a, &header_b].iter().all(|n| n.as_deref().is_some_and(|n| can_strip(n, strip)));
    if !names_given && (strip_failed || (saw_minus.is_none() && saw_plus.is_none() && !header_strippable)) {
        let unit = if strip == 1 { "component" } else { "components" };
        anyhow::bail!("error: git diff header lacks filename information when removing {} leading pathname {} (line {})", strip, unit, i + 1);
    }
    let default_old = header_a.map(|a| strip_components(&a, strip));
    let default_new = header_b.map(|b| strip_components(&b, strip));
    patch.old_path = rename_from.or(saw_minus.unwrap_or(default_old));
    patch.new_path = rename_to.or(saw_plus.unwrap_or(default_new));
    if patch.is_new {
        patch.old_path = None;
    }
    if patch.is_delete {
        patch.new_path = None;
    }
    if patch.old_path.is_none() && !patch.is_delete {
        patch.is_new = true;
    }
    if patch.new_path.is_none() && !patch.is_new {
        patch.is_delete = true;
    }

    let (hunks, next) = parse_hunks(lines, i)?;
    patch.hunks = hunks;
    Ok((patch, next))
}

fn parse_hunks(lines: &[&str], mut i: usize) -> anyhow::Result<(Vec<PatchHunk>, usize)> {
    let mut hunks = Vec::new();

    while i < lines.len() {
        let header = trim_cr(lines[i]);
        if !header.starts_with("@@ ") {
            break;
        }
        let (old_start, old_count, new_start, new_count) =
            parse_hunk_header(header).ok_or_else(|| anyhow::anyhow!("error: corrupt patch at line {}", i + 1))?;
        i += 1;

        let mut remaining_old = old_count;
        let mut remaining_new = new_count;
        let mut hunk_lines: Vec<HunkLine> = Vec::new();

        while remaining_old > 0 || remaining_new > 0 {
            if i >= lines.len() {
                anyhow::bail!("error: corrupt patch at line {}", i + 1);
            }
            let raw = lines[i];
            let (kind, body) = match raw.as_bytes().first() {
                Some(b' ') => (HunkLineKind::Context, &raw[1..]),
                None => (HunkLineKind::Context, ""),
                Some(b'\r') if raw == "\r" => (HunkLineKind::Context, ""),
                Some(b'-') => (HunkLineKind::Removed, &raw[1..]),
                Some(b'+') => (HunkLineKind::Added, &raw[1..]),
                Some(b'\\') => {
                    if let Some(last) = hunk_lines.last_mut() {
                        last.no_newline = true;
                    }
                    i += 1;
                    continue;
                }
                _ => anyhow::bail!("error: corrupt patch at line {}", i + 1),
            };

            match kind {
                HunkLineKind::Context if remaining_old > 0 && remaining_new > 0 => {
                    remaining_old -= 1;
                    remaining_new -= 1;
                }
                HunkLineKind::Removed if remaining_old > 0 => remaining_old -= 1,
                HunkLineKind::Added if remaining_new > 0 => remaining_new -= 1,
                _ => anyhow::bail!("error: corrupt patch at line {}", i + 1),
            }

            let had_cr = body.ends_with('\r');
            hunk_lines.push(HunkLine {
                kind,
                text: body.strip_suffix('\r').unwrap_or(body).to_string(),
                had_cr,
                no_newline: false,
            });
            i += 1;
        }

        if i < lines.len() && lines[i].starts_with('\\') {
            if let Some(last) = hunk_lines.last_mut() {
                last.no_newline = true;
            }
            i += 1;
        }

        hunks.push(PatchHunk { old_start, old_count, new_start, new_count, lines: hunk_lines });
    }

    Ok((hunks, i))
}

fn parse_hunk_header(header: &str) -> Option<(usize, usize, usize, usize)> {
    let mut parts = header.split_whitespace();
    parts.next()?;
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let parse_range = |r: &str| -> Option<(usize, usize)> {
        match r.split_once(',') {
            Some((s, c)) => Some((s.parse().ok()?, c.parse().ok()?)),
            None => Some((r.parse().ok()?, 1)),
        }
    };
    let (old_start, old_count) = parse_range(old)?;
    let (new_start, new_count) = parse_range(new)?;
    Some((old_start, old_count, new_start, new_count))
}

fn trim_cr(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

fn parse_mode(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim(), 8).ok()
}

fn git_side_name(name: &str, strip: usize) -> Option<String> {
    let name = name.split('\t').next().unwrap_or(name);
    if name == "/dev/null" {
        return None;
    }
    Some(strip_components(&unquote_path(name), strip))
}

fn git_side_name_checked(name: &str, strip: usize) -> (Option<String>, bool) {
    let bare = name.split('\t').next().unwrap_or(name);
    if bare == "/dev/null" {
        return (None, true);
    }
    (git_side_name(name, strip), can_strip(&unquote_path(bare), strip))
}

fn can_strip(path: &str, count: usize) -> bool {
    path.matches('/').count() >= count
}

fn traditional_name(name: &str, strip: usize) -> Option<String> {
    git_side_name(name.trim_end(), strip)
}

fn split_git_header_names(rest: &str) -> (Option<String>, Option<String>) {
    if rest.starts_with('"') {
        if let Some((first, remaining)) = take_quoted(rest) {
            let second = remaining.trim_start();
            let second = if second.starts_with('"') {
                take_quoted(second).map(|(s, _)| s).unwrap_or_default()
            } else {
                second.to_string()
            };
            return (Some(first), Some(second));
        }
    }
    for (pos, _) in rest.match_indices(' ') {
        let (left, right) = (&rest[..pos], &rest[pos + 1..]);
        if strip_components(left, 1) == strip_components(right, 1) {
            return (Some(left.to_string()), Some(right.to_string()));
        }
    }
    match rest.split_once(' ') {
        Some((a, b)) => (Some(a.to_string()), Some(b.to_string())),
        None => (Some(rest.to_string()), Some(rest.to_string())),
    }
}

fn take_quoted(text: &str) -> Option<(String, &str)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), &text[i + 1..])),
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                match bytes[i] {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'0'..=b'7' if i + 2 < bytes.len() => {
                        let octal = std::str::from_utf8(&bytes[i..i + 3]).ok()?;
                        out.push(u8::from_str_radix(octal, 8).ok()?);
                        i += 2;
                    }
                    other => out.push(other),
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    None
}

fn unquote_path(name: &str) -> String {
    if name.starts_with('"') {
        if let Some((unquoted, _)) = take_quoted(name) {
            return unquoted;
        }
    }
    name.to_string()
}

fn strip_components(path: &str, count: usize) -> String {
    let mut rest = path;
    for _ in 0..count {
        match rest.find('/') {
            Some(pos) => rest = &rest[pos + 1..],
            None => break,
        }
    }
    rest.to_string()
}

/// Splits file content into lines that remember their own terminator (`\n`, `\r\n`, or none
/// for a final line without newline).
// Used by `apply` to build the preimage a patch is applied to.
pub fn split_image(content: &str) -> Vec<PatchImageLine> {
    content
        .split_inclusive('\n')
        .map(|chunk| {
            if let Some(text) = chunk.strip_suffix("\r\n") {
                PatchImageLine { text: text.to_string(), eol: "\r\n".to_string() }
            } else if let Some(text) = chunk.strip_suffix('\n') {
                PatchImageLine { text: text.to_string(), eol: "\n".to_string() }
            } else {
                PatchImageLine { text: chunk.to_string(), eol: String::new() }
            }
        })
        .collect()
}

/// Joins image lines back into file content, each with its own terminator.
// Used by `apply` to produce the patched file content.
pub fn join_image(lines: &[PatchImageLine]) -> String {
    lines.iter().map(|l| format!("{}{}", l.text, l.eol)).collect()
}

/// Applies hunks in order to a file image, mirroring git's placement rules: each hunk's
/// preimage (context + removed lines) must match exactly, searching outward from the expected
/// line (`+1`, `-1`, `+2`, ...); a hunk starting at line 1 must match at the top of the file and
/// one without trailing context must match at the end. Line endings are compared loosely so
/// `rgit diff` output (which drops `\r`) applies to CRLF files; added lines take the file's
/// line ending. Returns the new image and `(hunk number, applied line, offset)` for every
/// hunk that applied away from its expected position.
// Used by `apply`.
pub fn apply_hunks(image: Vec<PatchImageLine>, hunks: &[PatchHunk]) -> Result<(Vec<PatchImageLine>, Vec<(usize, usize, isize)>), HunkFailure> {
    let crlf = image.first().is_some_and(|l| l.eol == "\r\n");
    let default_eol = if crlf { "\r\n" } else { "\n" };
    let mut img = image;
    let mut offsets = Vec::new();

    for (n, hunk) in hunks.iter().enumerate() {
        let preimage: Vec<&HunkLine> = hunk.lines.iter().filter(|l| l.kind != HunkLineKind::Added).collect();
        let trailing = hunk.lines.iter().rev().take_while(|l| l.kind == HunkLineKind::Context).count();
        let match_begin = hunk.old_start <= 1;
        let match_end = trailing == 0;
        let expected = hunk.new_start.saturating_sub(1);

        let pos = find_hunk_position(&img, &preimage, expected, match_begin, match_end).ok_or_else(|| HunkFailure {
            hunk_number: n + 1,
            old_start: hunk.old_start,
            searched_for: preimage.iter().map(|l| format!("{}\n", l.text)).collect(),
        })?;

        let mut replacement = Vec::new();
        let mut cursor = pos;
        for line in &hunk.lines {
            match line.kind {
                HunkLineKind::Context => {
                    let mut kept = img[cursor].clone();
                    if line.no_newline {
                        kept.eol.clear();
                    }
                    replacement.push(kept);
                    cursor += 1;
                }
                HunkLineKind::Removed => cursor += 1,
                HunkLineKind::Added => {
                    let eol = if line.no_newline {
                        ""
                    } else if line.had_cr {
                        "\r\n"
                    } else {
                        default_eol
                    };
                    replacement.push(PatchImageLine { text: line.text.clone(), eol: eol.to_string() });
                }
            }
        }
        img.splice(pos..pos + preimage.len(), replacement);

        if pos != expected {
            offsets.push((n + 1, pos + 1, pos as isize - expected as isize));
        }
    }

    let last = img.len().saturating_sub(1);
    for (idx, line) in img.iter_mut().enumerate() {
        if idx != last && line.eol.is_empty() {
            line.eol = default_eol.to_string();
        }
    }

    Ok((img, offsets))
}

fn find_hunk_position(img: &[PatchImageLine], preimage: &[&HunkLine], expected: usize, match_begin: bool, match_end: bool) -> Option<usize> {
    if preimage.len() > img.len() {
        return None;
    }
    let last = img.len() - preimage.len();
    let matches_at = |pos: usize| preimage.iter().enumerate().all(|(i, l)| img[pos + i].text == l.text);

    if match_begin || match_end {
        let pos = if match_begin { 0 } else { last };
        if match_begin && match_end && last != 0 {
            return None;
        }
        return matches_at(pos).then_some(pos);
    }

    let start = expected.min(last);
    if matches_at(start) {
        return Some(start);
    }
    for distance in 1..=last.max(1) {
        if start + distance <= last && matches_at(start + distance) {
            return Some(start + distance);
        }
        if distance <= start && matches_at(start - distance) {
            return Some(start - distance);
        }
    }
    None
}

/// Formats `apply --stat` output exactly like git: one ` name | count +++---` line per file
/// (names over 50 columns are shortened with `...`, the graph is scaled to fit 70 columns),
/// followed by the ` N files changed, X insertions(+), Y deletions(-)` summary.
// Used by `apply --stat`.
pub fn format_apply_stat(patches: &[FilePatch]) -> String {
    let max_len = patches.iter().map(|p| p.stat_name().len()).max().unwrap_or(0).min(50);
    let max_change = patches.iter().map(|p| { let (a, d) = p.line_counts(); a + d }).max().unwrap_or(0);
    let mut out = String::new();
    let (mut total_add, mut total_del) = (0, 0);

    for patch in patches {
        let mut name = patch.stat_name().to_string();
        if name.len() > max_len {
            let cut = name.len() + 3 - max_len;
            let from = name[cut..].find('/').map(|p| cut + p).unwrap_or(cut);
            name = format!("...{}", &name[from..]);
        }
        if patch.is_binary {
            out.push_str(&format!(" {:<width$} |  Bin\n", name, width = max_len));
            continue;
        }
        let (added, deleted) = patch.line_counts();
        total_add += added;
        total_del += deleted;

        let graph_width = if max_len + max_change > 70 { 70 - max_len } else { max_change };
        let (mut add, mut del) = (added, deleted);
        if max_change > 0 {
            let total = ((added + deleted) * graph_width + max_change / 2) / max_change;
            add = (added * graph_width + max_change / 2) / max_change;
            del = total - add;
        }
        out.push_str(&format!(" {:<width$} |{:>5} {}{}\n", name, added + deleted, "+".repeat(add), "-".repeat(del), width = max_len));
    }

    let files = patches.len();
    let mut summary = format!(" {} file{} changed", files, if files == 1 { "" } else { "s" });
    if total_add > 0 || total_del == 0 {
        summary.push_str(&format!(", {} insertion{}(+)", total_add, if total_add == 1 { "" } else { "s" }));
    }
    if total_del > 0 || total_add == 0 {
        summary.push_str(&format!(", {} deletion{}(-)", total_del, if total_del == 1 { "" } else { "s" }));
    }
    out.push_str(&summary);
    out.push('\n');
    out
}

/// Formats `apply --numstat` output: `added\tdeleted\tpath` per file (`-\t-\t` for binary).
// Used by `apply --numstat`.
pub fn format_apply_numstat(patches: &[FilePatch]) -> String {
    let mut out = String::new();
    for patch in patches {
        if patch.is_binary {
            out.push_str(&format!("-\t-\t{}\n", patch.stat_name()));
        } else {
            let (added, deleted) = patch.line_counts();
            out.push_str(&format!("{}\t{}\t{}\n", added, deleted, patch.stat_name()));
        }
    }
    out
}

/// Formats `apply --summary` output: creations, deletions, renames/copies (with git's
/// `dir/{old => new}` common-prefix form), and mode changes.
// Used by `apply --summary`.
pub fn format_apply_summary(patches: &[FilePatch]) -> String {
    let mut out = String::new();
    let file_mode_line = |verb: &str, mode: Option<u32>, name: &str| match mode {
        Some(m) => format!(" {} mode {:06o} {}\n", verb, m, name),
        None => format!(" {} {}\n", verb, name),
    };

    for patch in patches {
        if patch.is_new {
            out.push_str(&file_mode_line("create", patch.new_mode, patch.stat_name()));
        } else if patch.is_delete {
            out.push_str(&file_mode_line("delete", patch.old_mode, patch.display_name()));
        } else {
            let renamed = patch.is_rename || patch.is_copy;
            if renamed {
                let verb = if patch.is_rename { "rename" } else { "copy" };
                let old = patch.old_path.as_deref().unwrap_or("");
                let new = patch.new_path.as_deref().unwrap_or("");
                let mut common = 0;
                loop {
                    let (o, n) = (&old[common..], &new[common..]);
                    match (o.find('/'), n.find('/')) {
                        (Some(a), Some(b)) if a == b && o[..a] == n[..b] => common += a + 1,
                        _ => break,
                    }
                }
                let score = patch.similarity.unwrap_or(0);
                if common > 0 {
                    out.push_str(&format!(" {} {}{{{} => {}}} ({}%)\n", verb, &old[..common], &old[common..], &new[common..], score));
                } else {
                    out.push_str(&format!(" {} {} => {} ({}%)\n", verb, old, new, score));
                }
            }
            if let (Some(old_mode), Some(new_mode)) = (patch.old_mode, patch.new_mode) {
                if old_mode != new_mode {
                    if renamed {
                        out.push_str(&format!(" mode change {:06o} => {:06o}\n", old_mode, new_mode));
                    } else {
                        out.push_str(&format!(" mode change {:06o} => {:06o} {}\n", old_mode, new_mode, patch.stat_name()));
                    }
                }
            }
        }
    }
    out
}

/// Returns the git file mode of a working-tree file: `100755` when executable (Unix only),
/// otherwise `100644`.
// Used by `apply` when a patched file is not tracked in the index.
pub fn worktree_file_mode(path: &Path) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            if meta.permissions().mode() & 0o111 != 0 {
                return 0o100755;
            }
        }
    }
    let _ = path;
    0o100644
}

/// Writes patched content to the working tree, creating parent directories and applying the
/// executable bit for mode `100755` (Unix only).
// Used by `apply`.
pub fn write_worktree_file(path: &str, content: &str, mode: u32) -> anyhow::Result<()> {
    let target = Path::new(path);
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("Failed to create directory for {}", path))?;
        }
    }
    fs::write(target, content).with_context(|| format!("Failed to write {}", path))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = if mode == 0o100755 { 0o755 } else { 0o644 };
        fs::set_permissions(target, fs::Permissions::from_mode(perms))?;
    }
    let _ = mode;
    Ok(())
}

/// Deletes a file from the working tree and removes any parent directories left empty.
// Used by `apply` for deletion and rename patches.
pub fn remove_worktree_file(path: &str) -> anyhow::Result<()> {
    fs::remove_file(path).with_context(|| format!("Failed to remove {}", path))?;
    let mut dir = Path::new(path).parent();
    while let Some(d) = dir {
        if d.as_os_str().is_empty() || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
    Ok(())
}

/// Loads the current content and mode of a file a patch applies to: from earlier patches in
/// the same run (`pending`), the index (`--cached`), or the working tree. With `--index` the
/// working-tree file must also be tracked and match its index entry. Returns `Ok(None)` when
/// the file does not exist, and git-style error text when it cannot be used.
// Used by `apply`.
pub fn load_patch_target(path: &str, opts: &ApplyOptions, index_map: &HashMap<String, IndexEntry>, pending: &BTreeMap<String, Option<(String, u32)>>) -> Result<Option<(String, u32)>, String> {
    if let Some(state) = pending.get(path) {
        return Ok(state.clone());
    }

    let read_blob = |entry: &IndexEntry| -> Result<Vec<u8>, String> {
        read_object(&hex::encode(entry.hash)).map(|(_, data)| data).map_err(|e| format!("error: {}: {}", path, e))
    };
    let to_text = |bytes: Vec<u8>| -> Result<String, String> {
        String::from_utf8(bytes).map_err(|_| format!("error: {}: cannot apply a text patch to a binary file", path))
    };

    if opts.cached {
        return match index_map.get(path) {
            Some(entry) => Ok(Some((to_text(read_blob(entry)?)?, entry.mode))),
            None => Ok(None),
        };
    }

    let file = Path::new(path);
    if !file.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(file).map_err(|e| format!("error: {}: {}", path, e))?;

    if opts.index {
        let entry = match index_map.get(path) {
            Some(entry) => entry,
            None => return Ok(None),
        };
        if hash_content("blob", &bytes) != entry.hash {
            return Err(format!("error: {}: does not match index", path));
        }
    }

    let mode = index_map.get(path).map(|e| e.mode).unwrap_or_else(|| worktree_file_mode(file));
    Ok(Some((to_text(bytes)?, mode)))
}

/// Reports whether a patch's target path already exists, taking earlier patches in the same
/// run into account and checking the index or working tree depending on the apply mode.
// Used by `apply` before creating, renaming, or copying a file.
pub fn patch_target_exists(path: &str, opts: &ApplyOptions, index_map: &HashMap<String, IndexEntry>, pending: &BTreeMap<String, Option<(String, u32)>>) -> bool {
    if let Some(state) = pending.get(path) {
        return state.is_some();
    }
    if opts.cached {
        return index_map.contains_key(path);
    }
    Path::new(path).exists() || (opts.index && index_map.contains_key(path))
}

/// Builds an index entry for content that only exists in the index (`apply --cached`): the
/// working tree is untouched, so stat fields are zero and only hash, size, and path are set.
// Used by `apply --cached`.
pub fn cached_index_entry(path: &str, hash: [u8; 20], size: usize) -> IndexEntry {
    IndexEntry {
        ctime_secs: 0,
        ctime_nsecs: 0,
        mtime_secs: 0,
        mtime_nsecs: 0,
        dev: 0,
        ino: 0,
        mode: 0o100644,
        uid: 0,
        gid: 0,
        size: size as u32,
        hash,
        path: path.to_string(),
    }
}
