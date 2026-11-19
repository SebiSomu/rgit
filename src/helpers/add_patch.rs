//! Patch mode (`add -p`, `reset -p`, `checkout -p`, `restore -p`, `stash -p`): a port of git's
//! `add-patch.c`. Each file's diff is shown hunk by hunk with git's prompts and commands
//! (`y n q a d j J k K g / s e ?`); the hunks chosen for a file are applied to the mode's target
//! as soon as the file is done, as git does. Hunks are applied in memory (no `apply` round trip),
//! which also covers split hunks that share context lines and hunks edited by hand.

use std::io::{BufRead, Write};

use crate::helpers::format_patch::diff_file_contents;
use crate::objects::{HunkUse, InteractiveFile, InteractiveHunk, PatchModeSpec, PatchTarget};

const HELP_REMAINDER: &str = "j - leave this hunk undecided, see next undecided hunk\n\
J - leave this hunk undecided, see next hunk\n\
k - leave this hunk undecided, see previous undecided hunk\n\
K - leave this hunk undecided, see previous hunk\n\
g - select a hunk to go to\n\
/ - search for a hunk matching the given regex\n\
s - split the current hunk into smaller hunks\n\
e - manually edit the current hunk\n\
? - print help\n";

macro_rules! mode_spec {
    ($verb:literal, $target_words:literal, $hint:literal, $help:literal, $rev:expr, $target:expr) => {
        PatchModeSpec {
            prompts: [
                concat!($verb, " mode change", $target_words, " [y,n,q,a,d%s,?]? "),
                concat!($verb, " deletion", $target_words, " [y,n,q,a,d%s,?]? "),
                concat!($verb, " addition", $target_words, " [y,n,q,a,d%s,?]? "),
                concat!($verb, " this hunk", $target_words, " [y,n,q,a,d%s,?]? "),
            ],
            edit_hint: $hint,
            help: $help,
            is_reverse: $rev,
            target: $target,
        }
    };
}

/// `add -p`: stage hunks of the working tree into the index.
pub const MODE_ADD: PatchModeSpec = mode_spec!(
    "Stage", "",
    "If the patch applies cleanly, the edited hunk will immediately be marked for staging.",
    "y - stage this hunk\nn - do not stage this hunk\nq - quit; do not stage this hunk or any of the remaining ones\na - stage this hunk and all later hunks in the file\nd - do not stage this hunk or any of the later hunks in the file\n",
    false, PatchTarget::Index
);
/// `stash -p`: pick the hunks (HEAD vs working tree) that go into the stash.
pub const MODE_STASH: PatchModeSpec = mode_spec!(
    "Stash", "",
    "If the patch applies cleanly, the edited hunk will immediately be marked for stashing.",
    "y - stash this hunk\nn - do not stash this hunk\nq - quit; do not stash this hunk or any of the remaining ones\na - stash this hunk and all later hunks in the file\nd - do not stash this hunk or any of the later hunks in the file\n",
    false, PatchTarget::Stash
);
/// `reset -p` (HEAD): unstage hunks.
pub const MODE_RESET_HEAD: PatchModeSpec = mode_spec!(
    "Unstage", "",
    "If the patch applies cleanly, the edited hunk will immediately be marked for unstaging.",
    "y - unstage this hunk\nn - do not unstage this hunk\nq - quit; do not unstage this hunk or any of the remaining ones\na - unstage this hunk and all later hunks in the file\nd - do not unstage this hunk or any of the later hunks in the file\n",
    true, PatchTarget::Index
);
/// `reset -p <tree-ish>`: copy hunks of another tree into the index.
pub const MODE_RESET_NOTHEAD: PatchModeSpec = mode_spec!(
    "Apply", " to index",
    "If the patch applies cleanly, the edited hunk will immediately be marked for applying.",
    "y - apply this hunk to index\nn - do not apply this hunk to index\nq - quit; do not apply this hunk or any of the remaining ones\na - apply this hunk and all later hunks in the file\nd - do not apply this hunk or any of the later hunks in the file\n",
    false, PatchTarget::Index
);
/// `checkout -p` / `restore -p`: discard working-tree hunks (back to the index).
pub const MODE_CHECKOUT_INDEX: PatchModeSpec = mode_spec!(
    "Discard", " from worktree",
    "If the patch applies cleanly, the edited hunk will immediately be marked for discarding.",
    "y - discard this hunk from worktree\nn - do not discard this hunk from worktree\nq - quit; do not discard this hunk or any of the remaining ones\na - discard this hunk and all later hunks in the file\nd - do not discard this hunk or any of the later hunks in the file\n",
    true, PatchTarget::Worktree
);
/// `checkout -p HEAD` / `restore -p -SW`: discard hunks from index and working tree.
pub const MODE_CHECKOUT_HEAD: PatchModeSpec = mode_spec!(
    "Discard", " from index and worktree",
    "If the patch applies cleanly, the edited hunk will immediately be marked for discarding.",
    "y - discard this hunk from index and worktree\nn - do not discard this hunk from index and worktree\nq - quit; do not discard this hunk or any of the remaining ones\na - discard this hunk and all later hunks in the file\nd - do not discard this hunk or any of the later hunks in the file\n",
    true, PatchTarget::Both
);
/// `checkout -p <tree-ish>`: copy hunks of another tree into index and working tree.
pub const MODE_CHECKOUT_NOTHEAD: PatchModeSpec = mode_spec!(
    "Apply", " to index and worktree",
    "If the patch applies cleanly, the edited hunk will immediately be marked for applying.",
    "y - apply this hunk to index and worktree\nn - do not apply this hunk to index and worktree\nq - quit; do not apply this hunk or any of the remaining ones\na - apply this hunk and all later hunks in the file\nd - do not apply this hunk or any of the later hunks in the file\n",
    false, PatchTarget::Both
);
/// `restore -p --source=HEAD`: discard hunks from the working tree (back to HEAD).
pub const MODE_WORKTREE_HEAD: PatchModeSpec = mode_spec!(
    "Discard", " from worktree",
    "If the patch applies cleanly, the edited hunk will immediately be marked for discarding.",
    "y - discard this hunk from worktree\nn - do not discard this hunk from worktree\nq - quit; do not discard this hunk or any of the remaining ones\na - discard this hunk and all later hunks in the file\nd - do not discard this hunk or any of the later hunks in the file\n",
    true, PatchTarget::Worktree
);
/// `restore -p --source=<tree-ish>`: copy hunks of another tree into the working tree.
pub const MODE_WORKTREE_NOTHEAD: PatchModeSpec = mode_spec!(
    "Apply", " to worktree",
    "If the patch applies cleanly, the edited hunk will immediately be marked for applying.",
    "y - apply this hunk to worktree\nn - do not apply this hunk to worktree\nq - quit; do not apply this hunk or any of the remaining ones\na - apply this hunk and all later hunks in the file\nd - do not apply this hunk or any of the later hunks in the file\n",
    false, PatchTarget::Worktree
);

/// Builds one patch-mode file from both sides of a diff: the diff text is generated as `diff`
/// would show it (with `b/` and `a/` swapped for git's `-R` diffs), then cut into a header, an optional mode-change pseudo hunk, and the `@@`
/// hunks, with what git's `parse_diff` records (added, deleted, binary, splittable groups).
// Used by the `-p` modes of `add`, `reset`, `checkout`, `restore` and `stash`.
pub fn parse_patch_file(path: &str, old: Option<([u8; 20], u32)>, old_bytes: Vec<u8>, new: Option<([u8; 20], u32)>, new_bytes: Vec<u8>, swapped_prefixes: bool) -> anyhow::Result<InteractiveFile> {
    let diff = diff_file_contents(path, old, new, old_bytes.clone(), new_bytes.clone())?;
    let mut file = InteractiveFile { path: path.to_string(), old, new, old_bytes, new_bytes, ..Default::default() };
    let mut text = diff.text.into_bytes();
    if swapped_prefixes {
        // A diff git computes with -R (`<tree-ish>` modes) names the sides `b/` then `a/`.
        let s = String::from_utf8_lossy(&text).into_owned();
        let mut swapped = String::new();
        for (n, line) in s.split_inclusive('\n').enumerate() {
            let line = if n == 0 && line.starts_with("diff --git a/") {
                format!("diff --git b/{} a/{}\n", path, path)
            } else if let Some(rest) = line.strip_prefix("--- a/") {
                format!("--- b/{}", rest)
            } else if let Some(rest) = line.strip_prefix("+++ b/") {
                format!("+++ a/{}", rest)
            } else {
                line.to_string()
            };
            swapped.push_str(&line);
        }
        text = swapped.into_bytes();
    }
    let mut current: Option<InteractiveHunk> = None;
    let mut mode_lines: Vec<Vec<u8>> = Vec::new();
    for line in text.split_inclusive(|&b| b == b'\n') {
        if line.starts_with(b"@@ ") {
            if let Some(h) = current.take() {
                file.hunks.push(h);
            }
            current = Some(parse_hunk_header(line).unwrap_or_default());
            continue;
        }
        match current.as_mut() {
            Some(h) => h.lines.push(line.to_vec()),
            None => {
                if line.starts_with(b"old mode ") || line.starts_with(b"new mode ") {
                    mode_lines.push(line.to_vec());
                    continue;
                }
                if line.starts_with(b"new file") {
                    file.added = true;
                } else if line.starts_with(b"deleted file") {
                    file.deleted = true;
                } else if line.starts_with(b"GIT binary patch") || line.starts_with(b"Binary files") {
                    file.binary = true;
                    continue;
                }
                if !file.binary {
                    file.header.push(line.to_vec());
                }
            }
        }
    }
    if let Some(h) = current.take() {
        file.hunks.push(h);
    }
    if file.deleted && !file.binary {
        // The hunk of a deleted file starts at its `deleted file mode` line.
        let at = file.header.iter().position(|l| l.starts_with(b"deleted file")).unwrap_or(file.header.len());
        let mut lines: Vec<Vec<u8>> = file.header.split_off(at);
        let original = file.hunks.first().cloned().unwrap_or_default();
        if !file.hunks.is_empty() {
            let mut at_line = Vec::new();
            render_header(&original, &mut at_line);
            lines.push(at_line);
            lines.extend(original.lines.iter().cloned());
        }
        file.hunks = vec![InteractiveHunk { lines, raw: true, ..original }];
    }
    if file.binary {
        // Literal/delta lines of a binary patch are not part of the header.
        file.header.retain(|l| l.starts_with(b"diff --git") || l.starts_with(b"index ") || l.starts_with(b"new file") || l.starts_with(b"deleted file"));
        file.hunks.clear();
    }
    for hunk in &mut file.hunks {
        hunk.splittable_into = if hunk.raw { 1 } else { count_change_groups(&hunk.lines) };
    }
    if !mode_lines.is_empty() {
        file.mode_change = true;
        file.hunks.insert(0, InteractiveHunk { lines: mode_lines, mode_change: true, splittable_into: 0, ..Default::default() });
    }
    Ok(file)
}

/// Parses `@@ -a[,b] +c[,d] @@<extra>` (a missing count is 1).
fn parse_hunk_header(line: &[u8]) -> Option<InteractiveHunk> {
    let text = String::from_utf8_lossy(line);
    let rest = text.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, rest) = rest.split_once(" @@")?;
    let range = |s: &str| -> Option<(usize, usize)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (old_offset, old_count) = range(old)?;
    let (new_offset, new_count) = range(new)?;
    let extra_start = line.len() - rest.len();
    let extra = if rest == "\n" { Vec::new() } else { line[extra_start..].to_vec() };
    Some(InteractiveHunk { old_offset, old_count, new_offset, new_count, extra, ..Default::default() })
}

fn marker(line: &[u8]) -> u8 {
    match line.first() {
        Some(b'\n') | Some(b'\r') => b' ',
        Some(&c) => c,
        None => b' ',
    }
}

/// How many groups of `-`/`+` lines, separated by context, a hunk has.
fn count_change_groups(lines: &[Vec<u8>]) -> usize {
    let mut groups = 0;
    let mut in_change = false;
    for line in lines {
        match marker(line) {
            b'-' | b'+' => {
                if !in_change {
                    groups += 1;
                }
                in_change = true;
            }
            b'\\' => {}
            _ => in_change = false,
        }
    }
    groups
}

fn render_header(hunk: &InteractiveHunk, out: &mut Vec<u8>) {
    if hunk.mode_change || hunk.raw {
        return;
    }
    out.extend_from_slice(format!("@@ -{}", hunk.old_offset).as_bytes());
    if hunk.old_count != 1 {
        out.extend_from_slice(format!(",{}", hunk.old_count).as_bytes());
    }
    out.extend_from_slice(format!(" +{}", hunk.new_offset).as_bytes());
    if hunk.new_count != 1 {
        out.extend_from_slice(format!(",{}", hunk.new_count).as_bytes());
    }
    out.extend_from_slice(b" @@");
    if hunk.extra.is_empty() {
        out.push(b'\n');
    } else {
        out.extend_from_slice(&hunk.extra);
    }
}

/// A hunk as git prints it: its `@@` line (none for the mode-change pseudo hunk) and its lines.
// Used by patch mode's prompt loop and `checkout -p`'s last-resort output.
pub fn render_hunk(hunk: &InteractiveHunk) -> Vec<u8> {
    let mut out = Vec::new();
    render_header(hunk, &mut out);
    for line in &hunk.lines {
        out.extend_from_slice(line);
    }
    out
}

/// git's `split_hunk`: one hunk per change group; the context between two groups belongs to
/// both, and each part gets its own line numbers (only the first keeps the function context).
fn split_hunk(hunk: &InteractiveHunk) -> Vec<InteractiveHunk> {
    // Group the line indices: runs of context and runs of changes.
    let lines = &hunk.lines;
    let mut groups: Vec<(usize, usize)> = Vec::new(); // [start, end) of each change run
    let mut i = 0;
    while i < lines.len() {
        if matches!(marker(&lines[i]), b'-' | b'+') {
            let start = i;
            while i < lines.len() && matches!(marker(&lines[i]), b'-' | b'+' | b'\\') {
                i += 1;
            }
            groups.push((start, i));
        } else {
            i += 1;
        }
    }
    let mut parts = Vec::new();
    for (g, &(start, end)) in groups.iter().enumerate() {
        let from = if g == 0 { 0 } else { groups[g - 1].1 };
        let to = if g + 1 == groups.len() { lines.len() } else { groups[g + 1].0 };
        let (mut old_before, mut new_before) = (0, 0);
        for line in &lines[..from] {
            match marker(line) {
                b'-' => old_before += 1,
                b'+' => new_before += 1,
                b'\\' => {}
                _ => {
                    old_before += 1;
                    new_before += 1;
                }
            }
        }
        let part_lines: Vec<Vec<u8>> = lines[from..to].to_vec();
        let (mut old_count, mut new_count) = (0, 0);
        for line in &part_lines {
            match marker(line) {
                b'-' => old_count += 1,
                b'+' => new_count += 1,
                b'\\' => {}
                _ => {
                    old_count += 1;
                    new_count += 1;
                }
            }
        }
        let _ = (start, end);
        parts.push(InteractiveHunk {
            old_offset: if old_count == 0 && hunk.old_count == 0 { hunk.old_offset } else { hunk.old_offset + old_before },
            old_count,
            new_offset: if new_count == 0 && hunk.new_count == 0 { hunk.new_offset } else { hunk.new_offset + new_before },
            new_count,
            extra: if g == 0 { hunk.extra.clone() } else { Vec::new() },
            lines: part_lines,
            // The parts keep the hunk's answer (git copies it), so a decided hunk stays decided.
            use_: hunk.use_,
            splittable_into: 1,
            mode_change: false,
            raw: false,
        });
    }
    parts
}

/// Applies hunks (in order) to `target`: each hunk's old side (new side when `reverse`) must be
/// found at its line number, or the nearest place it matches, and is replaced by the other side.
/// Returns the 1-based line of the first hunk that does not apply.
// Used by patch mode to apply the chosen hunks, and by `stash -p` to take them out of the working tree.
pub fn apply_hunks(target: &[u8], hunks: &[&InteractiveHunk], reverse: bool) -> Result<Vec<u8>, usize> {
    let mut result: Vec<Vec<u8>> = target.split_inclusive(|&b| b == b'\n').map(|l| l.to_vec()).collect();
    let mut shift: isize = 0;
    let merged = merge_overlapping(hunks);
    for hunk in &merged {
        let (mut from, mut to): (Vec<Vec<u8>>, Vec<Vec<u8>>) = (Vec::new(), Vec::new());
        let mut last: Option<u8> = None; // which side(s) the previous line went to
        for line in &hunk.lines {
            let mut m = marker(line);
            if m == b'\\' {
                // "\ No newline at end of file": the previous line has no line end.
                let strip = |v: &mut Vec<Vec<u8>>| {
                    if let Some(l) = v.last_mut() {
                        if l.last() == Some(&b'\n') {
                            l.pop();
                        }
                    }
                };
                match last {
                    Some(b'-') => strip(&mut from),
                    Some(b'+') => strip(&mut to),
                    Some(_) => {
                        strip(&mut from);
                        strip(&mut to);
                    }
                    None => {}
                }
                continue;
            }
            if reverse {
                m = match m {
                    b'-' => b'+',
                    b'+' => b'-',
                    other => other,
                };
            }
            let body = if line.first() == Some(&b'\n') || line.starts_with(b"\r\n") { line.clone() } else { line[1..].to_vec() };
            match m {
                b'-' => from.push(body),
                b'+' => to.push(body),
                _ => {
                    from.push(body.clone());
                    to.push(body);
                }
            }
            last = Some(m);
        }
        let (offset, count) = if reverse { (hunk.new_offset, hunk.new_count) } else { (hunk.old_offset, hunk.old_count) };
        // git apply's rules: a hunk at the top (line 0 or 1) must match at the beginning, one
        // without trailing context at the end; otherwise the nearest place, forward first.
        let trailing = hunk.lines.iter().rev().filter(|l| marker(l) != b'\\').take_while(|l| !matches!(marker(l), b'-' | b'+')).count();
        let (match_beginning, match_end) = (offset <= 1, trailing == 0);
        let expected = if count == 0 { offset } else { offset.saturating_sub(1) } as isize + shift;
        let len = result.len() as isize;
        let fits = |at: isize| -> bool {
            at >= 0
                && (at as usize) + from.len() <= result.len()
                && (!match_beginning || at == 0)
                && (!match_end || at as usize + from.len() == result.len())
                && result[at as usize..at as usize + from.len()] == from[..]
        };
        let mut found = None;
        let start = expected.clamp(0, len);
        if fits(start) {
            found = Some(start);
        } else {
            let (mut back, mut fwd) = (start, start);
            while back > 0 || fwd < len {
                if fwd < len {
                    fwd += 1;
                    if fits(fwd) {
                        found = Some(fwd);
                        break;
                    }
                }
                if back > 0 {
                    back -= 1;
                    if fits(back) {
                        found = Some(back);
                        break;
                    }
                }
            }
        }
        let Some(at) = found else { return Err(offset) };
        let at = at as usize;
        result.splice(at..at + from.len(), to.iter().cloned());
        shift += to.len() as isize - from.len() as isize;
    }
    Ok(result.concat())
}

/// git's `merge_hunks`: hunks that overlap or touch on the old side (the parts of a split
/// hunk, for instance) become one hunk, their shared context lines kept once, before the patch
/// is checked or applied.
fn merge_overlapping(hunks: &[&InteractiveHunk]) -> Vec<InteractiveHunk> {
    let mut merged: Vec<InteractiveHunk> = Vec::new();
    for hunk in hunks.iter().filter(|h| !h.mode_change && !h.raw) {
        match merged.last_mut() {
            Some(last) if last.old_offset + last.old_count >= hunk.old_offset => {
                let overlap = (last.old_offset + last.old_count - hunk.old_offset).min(hunk.lines.len());
                let shared = hunk.lines[..overlap].iter().all(|l| marker(l) == b' ');
                let skip = if shared { overlap } else { 0 };
                last.lines.extend(hunk.lines[skip..].iter().cloned());
                last.old_count = hunk.old_offset + hunk.old_count - last.old_offset;
                last.new_count = last.lines.iter().filter(|l| matches!(marker(l), b' ' | b'+')).count();
            }
            _ => merged.push((*hunk).clone()),
        }
    }
    merged
}

/// The content a file's target ends up with: `None` when the chosen hunks delete it, otherwise
/// its bytes and mode. `current` is the target's present state (mode and bytes; `None` when the
/// target does not have the file), `place` names the target in errors ("working directory" or
/// "index"). An error carries the lines `git apply` would print.
// Used by patch mode when a file's hunks are decided, and by `stash -p`.
pub fn apply_patch_file(file: &InteractiveFile, mode: &PatchModeSpec, current: Option<(u32, &[u8])>, place: &str) -> Result<Option<(Vec<u8>, u32)>, String> {
    let used: Vec<&InteractiveHunk> = file.hunks.iter().filter(|h| h.use_ == HunkUse::Use).collect();
    let reverse = mode.is_reverse;
    // A whole-file creation or deletion, taken back or carried out.
    let (removes, creates) = if reverse { (file.added, file.deleted) } else { (file.deleted, file.added) };
    let everything = file.hunks.iter().all(|h| h.use_ == HunkUse::Use) && (!file.hunks.is_empty() || file.head_use == HunkUse::Use);
    if removes && everything {
        return match current {
            Some(_) => Ok(None),
            None => Err(format!("error: {}: does not exist in {}", file.path, place)),
        };
    }
    if creates && current.is_some() {
        return Err(format!("error: {}: already exists in {}", file.path, place));
    }
    let to_side = if reverse { file.old } else { file.new };
    let base: Vec<u8> = current.map(|(_, b)| b.to_vec()).unwrap_or_default();
    if creates && file.deleted {
        // Taking back a deletion restores the file as it was.
        return Ok(Some((file.old_bytes.clone(), file.old.map(|e| e.1).unwrap_or(0o100644))));
    }
    let content = apply_hunks(&base, &used, reverse).map_err(|line| format!("error: patch failed: {}:{}\nerror: {}: patch does not apply", file.path, line, file.path))?;
    let mut file_mode = current.map(|(m, _)| m).or(to_side.map(|e| e.1)).unwrap_or(0o100644);
    if creates {
        file_mode = to_side.map(|e| e.1).unwrap_or(file_mode);
    }
    if file.mode_change && file.hunks.first().is_some_and(|h| h.use_ == HunkUse::Use) {
        if let Some((_, m)) = to_side {
            file_mode = m;
        }
    }
    Ok(Some((content, file_mode)))
}

fn err(out: &mut std::io::StdoutLock, message: &str) {
    let _ = out.flush();
    eprintln!("{}", message);
}

/// Reads one answer line (trimmed); `None` at end of input.
fn read_answer(out: &mut std::io::StdoutLock, input: &mut dyn BufRead) -> Option<String> {
    let _ = out.flush();
    let mut line = String::new();
    match input.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_string()),
    }
}

fn prompt_yesno(out: &mut std::io::StdoutLock, input: &mut dyn BufRead, prompt: &str) -> Option<bool> {
    loop {
        let _ = write!(out, "{}", prompt);
        let answer = read_answer(out, input)?;
        match answer.chars().next().map(|c| c.to_ascii_lowercase()) {
            Some('y') => return Some(true),
            Some('n') => return Some(false),
            _ => {}
        }
    }
}

/// Runs patch mode over the files (git's `run_add_p`): each file is shown and decided, and its
/// chosen hunks are handed to `apply` right away. Prints git's "No changes." / "Only binary
/// files changed." when there is nothing to ask about. Returns whether the user quit.
// Used by the `-p` modes of `add`, `reset`, `checkout`, `restore` and `stash`.
pub fn run_patch_mode(
    mode: &PatchModeSpec,
    files: &mut [InteractiveFile],
    apply: &mut dyn FnMut(&InteractiveFile, &mut dyn FnMut(&str) -> Option<bool>) -> anyhow::Result<()>,
) -> anyhow::Result<bool> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut binary_count = 0;
    let mut quit = false;
    for file in files.iter_mut() {
        if file.binary && file.hunks.is_empty() {
            binary_count += 1;
            continue;
        }
        if patch_update_file(mode, file, &mut out, &mut input, apply)? {
            quit = true;
            break;
        }
    }
    let _ = out.flush();
    if files.is_empty() {
        eprintln!("No changes.");
    } else if binary_count == files.len() {
        eprintln!("Only binary files changed.");
    }
    Ok(quit)
}

/// git's `patch_update_file`: the prompt loop for one file, then applying its chosen hunks.
fn patch_update_file(
    mode: &PatchModeSpec,
    file: &mut InteractiveFile,
    out: &mut std::io::StdoutLock,
    input: &mut dyn BufRead,
    apply: &mut dyn FnMut(&InteractiveFile, &mut dyn FnMut(&str) -> Option<bool>) -> anyhow::Result<()>,
) -> anyhow::Result<bool> {
    if file.hunks.is_empty() && !file.added {
        return Ok(false);
    }
    let mut quit = false;
    for line in &file.header {
        let _ = out.write_all(line);
    }
    let mut hunk_index = 0usize;
    // As in git 2.43, a command once allowed for this file stays allowed (the flags only grow).
    let (mut allow_prev, mut allow_prev_undecided, mut allow_next, mut allow_next_undecided) = (false, false, false, false);
    let (mut allow_search, mut allow_split, mut allow_edit) = (false, false, false);
    loop {
        if hunk_index >= file.hunks.len() {
            hunk_index = 0;
        }
        let hunk_nr = file.hunks.len();
        let current_use = if hunk_nr > 0 { file.hunks[hunk_index].use_ } else { file.head_use };
        let undecided_previous = (0..hunk_index).rev().find(|&i| file.hunks[i].use_ == HunkUse::Undecided);
        let undecided_next = (hunk_index + 1..hunk_nr).find(|&i| file.hunks[i].use_ == HunkUse::Undecided);
        if undecided_previous.is_none() && undecided_next.is_none() && current_use != HunkUse::Undecided {
            break;
        }

        let mut options = String::new();
        if hunk_nr > 0 {
            let _ = out.write_all(&render_hunk(&file.hunks[hunk_index]));
            if undecided_previous.is_some() {
                allow_prev_undecided = true;
                options.push_str(",k");
            }
            if hunk_index > 0 {
                allow_prev = true;
                options.push_str(",K");
            }
            if undecided_next.is_some() {
                allow_next_undecided = true;
                options.push_str(",j");
            }
            if hunk_index + 1 < hunk_nr {
                allow_next = true;
                options.push_str(",J");
            }
            if hunk_nr > 1 {
                allow_search = true;
                options.push_str(",g,/");
            }
            if file.hunks[hunk_index].splittable_into > 1 {
                allow_split = true;
                options.push_str(",s");
            }
            if hunk_index + 1 > usize::from(file.mode_change) && !file.deleted {
                allow_edit = true;
                options.push_str(",e");
            }
        }
        let prompt = if file.deleted {
            mode.prompts[1]
        } else if file.added {
            mode.prompts[2]
        } else if file.mode_change && hunk_index == 0 {
            mode.prompts[0]
        } else {
            mode.prompts[3]
        };
        let _ = write!(out, "({}/{}) {}", hunk_index + 1, hunk_nr.max(1), prompt.replace("%s", &options));
        let Some(answer) = read_answer(out, input) else { break };
        let Some(first) = answer.chars().next() else { continue };
        let ch = first.to_ascii_lowercase();
        let set_current = |file: &mut InteractiveFile, idx: usize, value: HunkUse| {
            if file.hunks.is_empty() {
                file.head_use = value;
            } else {
                file.hunks[idx].use_ = value;
            }
        };
        let soft_increment = |undecided_next: Option<usize>| undecided_next.unwrap_or(hunk_nr);
        match ch {
            'y' => {
                set_current(file, hunk_index, HunkUse::Use);
                hunk_index = soft_increment(undecided_next);
            }
            'n' => {
                set_current(file, hunk_index, HunkUse::Skip);
                hunk_index = soft_increment(undecided_next);
            }
            'a' | 'd' | 'q' => {
                let value = if ch == 'a' { HunkUse::Use } else { HunkUse::Skip };
                if hunk_nr > 0 {
                    while hunk_index < hunk_nr {
                        if file.hunks[hunk_index].use_ == HunkUse::Undecided {
                            file.hunks[hunk_index].use_ = value;
                        }
                        hunk_index += 1;
                    }
                } else if file.head_use == HunkUse::Undecided {
                    file.head_use = value;
                }
                if ch == 'q' {
                    quit = true;
                    break;
                }
            }
            _ if first == 'K' => {
                if allow_prev {
                    hunk_index = hunk_index.wrapping_sub(1);
                } else {
                    err(out, "No previous hunk");
                }
            }
            _ if first == 'J' => {
                if allow_next {
                    hunk_index = hunk_index.wrapping_add(1);
                } else {
                    err(out, "No next hunk");
                }
            }
            _ if first == 'k' => {
                if allow_prev_undecided {
                    hunk_index = undecided_previous.unwrap_or(usize::MAX);
                } else {
                    err(out, "No previous hunk");
                }
            }
            _ if first == 'j' => {
                if allow_next_undecided {
                    hunk_index = undecided_next.unwrap_or(usize::MAX);
                } else {
                    err(out, "No next hunk");
                }
            }
            _ if first == 'g' => {
                if !allow_search {
                    err(out, "No other hunks to goto");
                    continue;
                }
                let mut response = answer[1..].trim().to_string();
                let mut i = hunk_index.saturating_sub(10).max(usize::from(file.mode_change));
                while response.is_empty() {
                    i = display_hunks(out, file, i);
                    let _ = write!(out, "{}", if i < hunk_nr { "go to which hunk (<ret> to see more)? " } else { "go to which hunk? " });
                    match read_answer(out, input) {
                        Some(r) => response = r,
                        // As in git, end of input leaves the answer empty (an invalid number).
                        None => break,
                    }
                }
                match response.parse::<usize>() {
                    Ok(n) if n > 0 && n <= hunk_nr => hunk_index = n - 1,
                    Ok(_) if response.chars().all(|c| c.is_ascii_digit()) => {
                        err(out, &format!("Sorry, only {} hunk{} available.", hunk_nr, if hunk_nr == 1 { "" } else { "s" }))
                    }
                    _ => err(out, &format!("Invalid number: '{}'", response)),
                }
            }
            _ if first == '/' => {
                if !allow_search {
                    err(out, "No other hunks to search");
                    continue;
                }
                let mut pattern = answer[1..].to_string();
                if pattern.is_empty() {
                    let _ = write!(out, "search for regex? ");
                    match read_answer(out, input) {
                        Some(r) if !r.is_empty() => pattern = r,
                        Some(_) => continue,
                        None => break,
                    }
                }
                let regex = match regex::RegexBuilder::new(&pattern).multi_line(true).build() {
                    Ok(r) => r,
                    Err(e) => {
                        err(out, &format!("Malformed search regexp {}: {}", pattern, e));
                        continue;
                    }
                };
                let mut i = hunk_index;
                loop {
                    if regex.is_match(&String::from_utf8_lossy(&render_hunk(&file.hunks[i]))) {
                        break;
                    }
                    i = (i + 1) % hunk_nr;
                    if i == hunk_index {
                        err(out, "No hunk matches the given pattern");
                        break;
                    }
                }
                hunk_index = i;
            }
            _ if first == 's' => {
                let hunk = file.hunks.get(hunk_index);
                match hunk {
                    Some(h) if allow_split && h.mode_change => {
                        // git "splits" the mode-change pseudo hunk into nothing and keeps it.
                        let _ = writeln!(out, "Split into 0 hunks.");
                    }
                    Some(h) if allow_split => {
                        let count = h.splittable_into;
                        let parts = split_hunk(h);
                        if !parts.is_empty() {
                            file.hunks.splice(hunk_index..=hunk_index, parts);
                        }
                        let _ = writeln!(out, "Split into {} hunks.", count);
                    }
                    _ => err(out, "Sorry, cannot split this hunk"),
                }
            }
            _ if first == 'e' => {
                if !allow_edit || file.hunks[hunk_index].mode_change {
                    err(out, "Sorry, cannot edit this hunk");
                } else if edit_hunk_loop(mode, file, hunk_index, out, input)? {
                    file.hunks[hunk_index].use_ = HunkUse::Use;
                    hunk_index = soft_increment(undecided_next);
                }
            }
            _ => {
                let _ = write!(out, "{}", mode.help);
                for line in HELP_REMAINDER.lines() {
                    let c = line.chars().next().unwrap_or('?');
                    if c != '?' && !options.contains(c) {
                        continue;
                    }
                    let _ = writeln!(out, "{}", line);
                }
            }
        }
    }

    let any_used = file.hunks.iter().any(|h| h.use_ == HunkUse::Use) || (file.hunks.is_empty() && file.head_use == HunkUse::Use);
    if any_used {
        let _ = out.flush();
        let mut ask = |prompt: &str| prompt_yesno(out, input, prompt);
        apply(file, &mut ask)?;
    }
    let _ = writeln!(out);
    Ok(quit)
}

/// git's `display_hunks` for `g`: up to 20 one-line summaries from `start`.
fn display_hunks(out: &mut std::io::StdoutLock, file: &InteractiveFile, start: usize) -> usize {
    let end = (start + 20).min(file.hunks.len());
    for i in start..end {
        let hunk = &file.hunks[i];
        let mark = match hunk.use_ {
            HunkUse::Use => '+',
            HunkUse::Skip => '-',
            HunkUse::Undecided => ' ',
        };
        let mut line = format!("{}{:2}: ", mark, i + 1);
        let header_start = line.len();
        line.push_str(&format!(" -{},{} +{},{} ", hunk.old_offset, hunk.old_count, hunk.new_offset, hunk.new_count));
        while line.len() - header_start < 20 {
            line.push(' ');
        }
        if let Some(first_change) = hunk.lines.iter().find(|l| l.first() != Some(&b' ')) {
            line.push_str(&String::from_utf8_lossy(first_change));
        }
        if line.len() - header_start > 80 {
            let mut cut = header_start + 80;
            while !line.is_char_boundary(cut) {
                cut -= 1;
            }
            line.truncate(cut);
        }
        if !line.ends_with('\n') {
            line.push('\n');
        }
        let _ = out.write_all(line.as_bytes());
    }
    end
}

/// git's `edit_hunk_loop`: lets the user edit the hunk until it applies with the other chosen
/// hunks of the file, or they give up. Returns whether an edited hunk was accepted.
fn edit_hunk_loop(mode: &PatchModeSpec, file: &mut InteractiveFile, index: usize, out: &mut std::io::StdoutLock, input: &mut dyn BufRead) -> anyhow::Result<bool> {
    let backup = file.hunks[index].clone();
    loop {
        let edited = edit_hunk_manually(mode, &backup)?;
        let Some(result) = edited else {
            // Everything removed: the edit is abandoned and the hunk left as it was.
            file.hunks[index] = backup;
            return Ok(false);
        };
        if let Some(mut hunk) = result {
            hunk.use_ = HunkUse::Use;
            let saved = std::mem::replace(&mut file.hunks[index], hunk);
            // git's run_apply_check: the whole file's patch, every hunk included, as `apply --check`.
            let all: Vec<&InteractiveHunk> = file.hunks.iter().filter(|h| !h.mode_change).collect();
            let base = if mode.is_reverse { &file.new_bytes } else { &file.old_bytes };
            let corrupt_at = corrupt_patch_line(file);
            let outcome = match corrupt_at {
                Some(line) => Err(format!("error: corrupt patch at line {}", line)),
                None => apply_hunks(base, &all, mode.is_reverse)
                    .map(|_| ())
                    .map_err(|line| format!("error: patch failed: {}:{}\nerror: {}: patch does not apply", file.path, line, file.path)),
            };
            match outcome {
                Ok(()) => {
                    file.hunks[index].use_ = saved.use_;
                    return Ok(true);
                }
                Err(message) => {
                    let _ = out.flush();
                    eprintln!("{}", message);
                    // git's message, whatever the mode's apply options are.
                    eprintln!("error: 'git apply --cached' failed");
                }
            }
            file.hunks[index] = saved;
        }
        file.hunks[index] = backup.clone();
        match prompt_yesno(out, input, "Your edited hunk does not apply. Edit again (saying \"no\" discards!) [y/n]? ") {
            Some(true) => continue,
            _ => return Ok(false),
        }
    }
}

/// `apply` without `--recount` rejects a hunk that changes nothing; this is the line of the
/// patch (header, then every hunk with its `@@` line) where it would stop, if any hunk does.
fn corrupt_patch_line(file: &InteractiveFile) -> Option<usize> {
    let all: Vec<&InteractiveHunk> = file.hunks.iter().collect();
    let mut line = file.header.len();
    for hunk in file.hunks.iter().filter(|h| h.raw) {
        line += hunk.lines.len();
    }
    for hunk in merge_overlapping(&all) {
        line += 1 + hunk.lines.len();
        if !hunk.lines.iter().any(|l| matches!(marker(l), b'-' | b'+')) {
            return Some(line + 1);
        }
    }
    None
}

/// Writes the hunk with git's instructions to a file, runs the editor, and reads it back:
/// `None` when everything was deleted, `Some(None)` when the `@@` line cannot be parsed,
/// otherwise the edited hunk with its counts recomputed.
fn edit_hunk_manually(mode: &PatchModeSpec, hunk: &InteractiveHunk) -> anyhow::Result<Option<Option<InteractiveHunk>>> {
    let (remove_minus, remove_plus) = if mode.is_reverse { ('+', '-') } else { ('-', '+') };
    let mut text: Vec<u8> = b"# Manual hunk edit mode -- see bottom for a quick guide.\n".to_vec();
    text.extend_from_slice(&render_hunk(hunk));
    let comment = format!(
        "---\nTo remove '{}' lines, make them ' ' lines (context).\nTo remove '{}' lines, delete them.\nLines starting with # will be removed.\n{}\nIf it does not apply cleanly, you will be given an opportunity to\nedit again.  If all lines of the hunk are removed, then the edit is\naborted and the hunk is left unchanged.\n",
        remove_minus, remove_plus, mode.edit_hint
    );
    for line in comment.lines() {
        text.extend_from_slice(if line.is_empty() { b"#\n".to_vec() } else { format!("# {}\n", line).into_bytes() }.as_slice());
    }
    let path = crate::core::repo::path(".git/addp-hunk-edit.diff");
    std::fs::write(&path, &text)?;
    crate::helpers::rebase::run_editor(&path, false)?;
    let edited = std::fs::read(&path)?;
    let _ = std::fs::remove_file(&path);

    let lines: Vec<Vec<u8>> = edited.split_inclusive(|&b| b == b'\n').filter(|l| l.first() != Some(&b'#')).map(|l| l.to_vec()).collect();
    if lines.is_empty() {
        return Ok(None);
    }
    let mut new_hunk = hunk.clone();
    let mut body = &lines[..];
    if lines[0].first() == Some(&b'@') {
        match parse_hunk_header(&lines[0]) {
            Some(h) => {
                new_hunk.old_offset = h.old_offset;
                new_hunk.new_offset = h.new_offset;
                new_hunk.extra = h.extra;
            }
            None => {
                eprintln!("error: could not parse hunk header");
                return Ok(Some(None));
            }
        }
        body = &lines[1..];
    }
    new_hunk.lines = body.to_vec();
    new_hunk.old_count = 0;
    new_hunk.new_count = 0;
    for line in &new_hunk.lines {
        match marker(line) {
            b'-' => new_hunk.old_count += 1,
            b'+' => new_hunk.new_count += 1,
            b' ' => {
                new_hunk.old_count += 1;
                new_hunk.new_count += 1;
            }
            _ => {}
        }
    }
    new_hunk.splittable_into = count_change_groups(&new_hunk.lines);
    Ok(Some(Some(new_hunk)))
}

#[cfg(test)]
#[path = "../../tests/unit/helpers/add_patch.rs"]
mod tests;
