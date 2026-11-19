//! Combined diffs (`diff --cc`) of unmerged paths against the working tree, ported from
//! git's `combine-diff.c`: the working-tree file is the result, stage 2 (ours) and stage 3
//! (theirs) are the two parents. Each parent is diffed against the result without context;
//! result lines a parent lacks get a `+` in that parent's column, and lines a parent had that
//! the result lost are hung (merged across parents by LCS) in front of the result line that
//! follows them, with a `-` in each column of a parent that had them. Dense mode then drops
//! hunks where the result simply took one side, and hunks get 3 lines of context.

use crate::core::object_store::abbreviate_hash_min;
use crate::diff::plumbing::{diff_path_selected, quote_c_path};
use crate::diff::hunks::{compacted_changes, split_diff_lines};
use crate::core::object_store::read_object;
use crate::core::index::IndexEntry;

const CONTEXT: usize = 3;
const NUM_PARENT: usize = 2;
const ALL_MASK: u32 = (1 << NUM_PARENT) - 1;
const MARK: u32 = 1 << NUM_PARENT;
const NO_PRE_DELETE: u32 = 2 << NUM_PARENT;
const NO_EOL_MARK_STRIP: fn(&str) -> &str = |line| line.strip_suffix(crate::diff::hunks::NO_EOL_MARK).unwrap_or(line);

/// A line some parents had and the result lost, with the set of those parents.
#[derive(Clone)]
struct LostLine {
    text: String,
    parent_map: u32,
}

/// One result line (plus the sentinel after the last one): which parents lack it (`flag`'s
/// low bits, then the mark bits), the lines lost in front of it, and each parent's line
/// number at this point.
#[derive(Clone, Default)]
struct SLine {
    text: Option<String>,
    flag: u32,
    lost: Vec<LostLine>,
    pending_lost: Vec<LostLine>,
    p_lno: [usize; NUM_PARENT],
}

/// The combined diffs of every unmerged path (selected by `specs`) that has both an "ours"
/// and a "theirs" stage, against its working-tree file, in index order.
// Used by `diff-files` and `diff` when a patch is shown and no stage was chosen.
pub fn combined_worktree_diffs(index: &[IndexEntry], specs: &[String]) -> anyhow::Result<String> {
    let mut out = String::new();
    let mut i = 0;
    while i < index.len() {
        let entry = &index[i];
        let end = index[i..].iter().position(|e| e.path != entry.path).map_or(index.len(), |n| i + n);
        let stages = &index[i..end];
        i = end;
        if entry.stage == 0 || !diff_path_selected(&entry.path, false, specs) {
            continue;
        }
        let (Some(ours), Some(theirs)) = (stages.iter().find(|e| e.stage == 2), stages.iter().find(|e| e.stage == 3)) else {
            continue;
        };
        out.push_str(&combined_diff(&entry.path, entry.mode, [ours, theirs])?);
    }
    Ok(out)
}

fn combined_diff(path: &str, first_mode: u32, parents: [&IndexEntry; NUM_PARENT]) -> anyhow::Result<String> {
    // The result: the working-tree file, or nothing (mode 0) when it is gone.
    let (result, mode) = match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => (std::fs::read(path)?, crate::workspace::index_plumbing::worktree_file_mode(&meta, Some(first_mode))),
        _ => (Vec::new(), 0),
    };
    let deleted = mode == 0;
    let parent_bytes: Vec<Vec<u8>> = parents.iter().map(|p| read_object(&hex::encode(p.hash)).map(|(_, b)| b)).collect::<anyhow::Result<_>>()?;
    let mode_differs = parents.iter().any(|p| p.mode != mode);

    let mut out = format!("diff --cc {}\nindex ", quote_c_path(path));
    for (n, parent) in parents.iter().enumerate() {
        out.push_str(if n > 0 { "," } else { "" });
        out.push_str(&abbreviate_hash_min(&hex::encode(parent.hash), 7));
    }
    out.push_str("..0000000\n");
    if mode_differs {
        // A parent of a working-tree combined diff is never "added", so no "new file mode".
        if deleted {
            out.push_str("deleted file ");
        }
        out.push_str("mode ");
        out.push_str(&parents.iter().map(|p| format!("{:06o}", p.mode)).collect::<Vec<_>>().join(","));
        if !deleted {
            out.push_str(&format!("..{:06o}", mode));
        }
        out.push('\n');
    }

    let is_binary = |b: &[u8]| b[..b.len().min(8000)].contains(&0);
    if is_binary(&result) || parent_bytes.iter().any(|b| is_binary(b)) {
        out.push_str("Binary files differ\n");
        return Ok(out);
    }
    out.push_str(&format!("--- {}\n", quote_c_path(&format!("a/{}", path))));
    out.push_str(&if deleted { "+++ /dev/null\n".to_string() } else { format!("+++ {}\n", quote_c_path(&format!("b/{}", path))) });
    if deleted {
        return Ok(out);
    }

    let result_lines = split_diff_lines(&result);
    let cnt = result_lines.len();
    let mut sline: Vec<SLine> = vec![SLine::default(); cnt + 2];
    for (n, line) in result_lines.iter().enumerate() {
        sline[n].text = Some(NO_EOL_MARK_STRIP(line).to_string());
    }
    for n in 0..NUM_PARENT {
        let reused = (0..n).find(|&j| parents[j].hash == parents[n].hash);
        match reused {
            Some(j) => reuse_combine_diff(&mut sline, n, j),
            None => combine_diff(&mut sline, cnt, &split_diff_lines(&parent_bytes[n]), &result_lines, n),
        }
    }
    make_hunks(&mut sline, cnt);
    dump_sline(&sline, cnt, &mut out);
    Ok(out)
}

/// Diffs one parent against the result and records it in `sline`: `+` flags for result lines
/// the parent lacks, lost lines hung on the next result line (coalesced with the other
/// parents' lost lines there), and this parent's line numbers.
fn combine_diff(sline: &mut [SLine], cnt: usize, parent: &[String], result: &[String], n: usize) {
    let nmask = 1u32 << n;
    let (changes, _, _) = compacted_changes(parent, result, true);
    for (old_start, old_count, new_start, new_count) in changes {
        // Removed lines hang on the first result line of the change, or (when nothing was
        // added) on the line after them.
        let bucket = new_start;
        for line in &parent[old_start..old_start + old_count] {
            sline[bucket].pending_lost.push(LostLine { text: NO_EOL_MARK_STRIP(line).to_string(), parent_map: nmask });
        }
        for flag_line in &mut sline[new_start..new_start + new_count] {
            flag_line.flag |= nmask;
        }
    }

    let mut p_lno = 1;
    for lno in 0..=cnt {
        sline[lno].p_lno[n] = p_lno;
        if !sline[lno].pending_lost.is_empty() {
            let pending = std::mem::take(&mut sline[lno].pending_lost);
            let base = std::mem::take(&mut sline[lno].lost);
            sline[lno].lost = coalesce_lines(base, pending, n);
        }
        p_lno += sline[lno].lost.iter().filter(|l| l.parent_map & nmask != 0).count();
        if lno < cnt && sline[lno].flag & nmask == 0 {
            p_lno += 1;
        }
    }
    sline[cnt + 1].p_lno[n] = p_lno;
}

/// A parent identical to an earlier one gets that parent's flags, lost lines and numbers.
fn reuse_combine_diff(sline: &mut [SLine], n: usize, j: usize) {
    let (jmask, nmask) = (1u32 << j, 1u32 << n);
    for line in sline.iter_mut() {
        line.p_lno[n] = line.p_lno[j];
        if line.flag & jmask != 0 {
            line.flag |= nmask;
        }
        for lost in &mut line.lost {
            if lost.parent_map & jmask != 0 {
                lost.parent_map |= nmask;
            }
        }
    }
}

/// Merges a parent's lost lines into the lost lines already hung on a result line: lines in
/// their longest common subsequence get the parent's bit, the others are inserted in order.
fn coalesce_lines(mut base: Vec<LostLine>, new: Vec<LostLine>, parent: usize) -> Vec<LostLine> {
    if new.is_empty() {
        return base;
    }
    if base.is_empty() {
        return new;
    }
    #[derive(Clone, Copy, PartialEq)]
    enum Dir {
        Base,
        New,
        Match,
    }
    let (bl, nl) = (base.len(), new.len());
    let mut lcs = vec![vec![0usize; nl + 1]; bl + 1];
    let mut dir = vec![vec![Dir::Base; nl + 1]; bl + 1];
    for d in dir[0].iter_mut().skip(1) {
        *d = Dir::New;
    }
    for i in 1..=bl {
        for j in 1..=nl {
            if base[i - 1].text == new[j - 1].text {
                lcs[i][j] = lcs[i - 1][j - 1] + 1;
                dir[i][j] = Dir::Match;
            } else if lcs[i][j - 1] >= lcs[i - 1][j] {
                lcs[i][j] = lcs[i][j - 1];
                dir[i][j] = Dir::New;
            } else {
                lcs[i][j] = lcs[i - 1][j];
                dir[i][j] = Dir::Base;
            }
        }
    }
    let (mut i, mut j) = (bl, nl);
    while i != 0 || j != 0 {
        match dir[i][j] {
            Dir::Match => {
                base[i - 1].parent_map |= 1 << parent;
                i -= 1;
                j -= 1;
            }
            Dir::New => {
                // Inserted right after the base line we're at (the lines before it are the
                // untouched base lines 0..i).
                base.insert(i, new[j - 1].clone());
                j -= 1;
            }
            Dir::Base => i -= 1,
        }
    }
    base
}

fn interesting(line: &SLine) -> bool {
    line.flag & ALL_MASK != 0 || !line.lost.is_empty()
}

/// `i` is the first uninteresting line; a hunk whose last line is only there for its lost
/// lines doesn't need it as trailing context.
fn adjust_hunk_tail(sline: &[SLine], hunk_begin: usize, i: usize) -> usize {
    if hunk_begin < i && sline[i - 1].flag & ALL_MASK == 0 { i - 1 } else { i }
}

fn find_next(sline: &[SLine], mut i: usize, cnt: usize, uninteresting: bool) -> usize {
    while i <= cnt {
        if (sline[i].flag & MARK == 0) == uninteresting {
            return i;
        }
        i += 1;
    }
    i
}

/// Marks the lines to show: interesting ones, minus (dense mode) hunks where the result just
/// took one side's version, plus context.
fn make_hunks(sline: &mut [SLine], cnt: usize) {
    for line in sline.iter_mut().take(cnt + 1) {
        if interesting(line) {
            line.flag |= MARK;
        } else {
            line.flag &= !MARK;
        }
    }
    let mut i = 0;
    while i <= cnt {
        while i <= cnt && sline[i].flag & MARK == 0 {
            i += 1;
        }
        if cnt < i {
            break;
        }
        let hunk_begin = i;
        let mut j = i + 1;
        while j <= cnt {
            if sline[j].flag & MARK == 0 {
                // Is there an interesting line within context span after this hunk?
                let mut la = adjust_hunk_tail(sline, hunk_begin, j);
                la = if la + CONTEXT < cnt + 1 { la + CONTEXT } else { cnt + 1 };
                let mut contin = false;
                while la > 0 && j < la {
                    la -= 1;
                    if sline[la].flag & MARK != 0 {
                        contin = true;
                        break;
                    }
                }
                if !contin {
                    break;
                }
                j = la;
            }
            j += 1;
        }
        let hunk_end = j;

        // Only two versions, and the result matches one of them: not interesting.
        let mut same_diff = 0;
        let mut has_interesting = false;
        let mut k = i;
        while k < hunk_end && !has_interesting {
            let this_diff = sline[k].flag & ALL_MASK;
            if this_diff != 0 {
                if same_diff == 0 {
                    same_diff = this_diff;
                } else if same_diff != this_diff {
                    has_interesting = true;
                    break;
                }
            }
            for lost in &sline[k].lost {
                if has_interesting {
                    break;
                }
                if same_diff == 0 {
                    same_diff = lost.parent_map;
                } else if same_diff != lost.parent_map {
                    has_interesting = true;
                }
            }
            k += 1;
        }
        if !has_interesting && same_diff != ALL_MASK {
            for line in &mut sline[hunk_begin..hunk_end] {
                line.flag &= !MARK;
            }
        }
        i = hunk_end;
    }
    give_context(sline, cnt);
}

fn give_context(sline: &mut [SLine], cnt: usize) {
    let mut i = find_next(sline, 0, cnt, false);
    if cnt < i {
        return;
    }
    while i <= cnt {
        // A few lines of leading context.
        let mut j = i.saturating_sub(CONTEXT);
        while j < i {
            if sline[j].flag & MARK == 0 {
                sline[j].flag |= NO_PRE_DELETE;
            }
            sline[j].flag |= MARK;
            j += 1;
        }
        loop {
            let j = find_next(sline, i, cnt, true);
            if cnt < j {
                return;
            }
            let k = find_next(sline, j, cnt, false);
            let mut j = adjust_hunk_tail(sline, i, j);
            if k < j + CONTEXT {
                // A small gap between interesting groups is shown too.
                while j < k {
                    sline[j].flag |= MARK;
                    j += 1;
                }
                i = k;
                continue;
            }
            // Trailing context.
            i = k;
            let end = if j + CONTEXT < cnt + 1 { j + CONTEXT } else { cnt + 1 };
            while j < end {
                sline[j].flag |= MARK;
                j += 1;
            }
            break;
        }
    }
}

/// git's hunk header comment: a line starting with a letter, `_` or `$`.
fn hunk_comment_line(text: &Option<String>) -> bool {
    text.as_ref().and_then(|t| t.bytes().next()).is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c == b'$')
}

fn dump_sline(sline: &[SLine], cnt: usize, out: &mut String) {
    let mut lno = 0;
    loop {
        let mut hunk_comment: Option<&str> = None;
        while lno <= cnt && sline[lno].flag & MARK == 0 {
            if hunk_comment_line(&sline[lno].text) {
                hunk_comment = sline[lno].text.as_deref();
            }
            lno += 1;
        }
        if cnt < lno {
            break;
        }
        let mut hunk_end = lno + 1;
        while hunk_end <= cnt && sline[hunk_end].flag & MARK != 0 {
            hunk_end += 1;
        }
        let mut rlines = hunk_end - lno;
        if cnt < hunk_end {
            rlines -= 1; // pointing at the last delete hunk
        }
        out.push_str("@@@");
        for n in 0..NUM_PARENT {
            let (l0, l1) = (sline[lno].p_lno[n], sline[hunk_end].p_lno[n]);
            out.push_str(&format!(" -{},{}", l0, l1 - l0));
        }
        out.push_str(&format!(" +{},{} @@@", lno + 1, rlines));
        if let Some(comment) = hunk_comment {
            // git prints the first 40 bytes up to (but, as it happens, not including) the last
            // non-space one.
            let bytes = comment.as_bytes();
            let mut comment_end = 0;
            for (k, &c) in bytes.iter().take(40).enumerate() {
                if c == b'\n' || c == 0 {
                    break;
                }
                if !c.is_ascii_whitespace() {
                    comment_end = k;
                }
            }
            if comment_end > 0 {
                out.push(' ');
                out.push_str(&String::from_utf8_lossy(&bytes[..comment_end]));
            }
        }
        out.push('\n');
        while lno < hunk_end {
            let sl = &sline[lno];
            lno += 1;
            if sl.flag & NO_PRE_DELETE == 0 {
                for lost in &sl.lost {
                    for n in 0..NUM_PARENT {
                        out.push(if lost.parent_map & (1 << n) != 0 { '-' } else { ' ' });
                    }
                    out.push_str(&lost.text);
                    out.push('\n');
                }
            }
            if cnt < lno {
                break;
            }
            for n in 0..NUM_PARENT {
                out.push(if sl.flag & (1 << n) != 0 { '+' } else { ' ' });
            }
            out.push_str(sl.text.as_deref().unwrap_or(""));
            out.push('\n');
        }
    }
}
