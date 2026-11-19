use std::collections::HashMap;
use crate::core::commit::split_commit_message;
use crate::diff::hunks::{compacted_changes, diff_commit_against_parent};
use crate::history::rev_list::engine::{parse_revision_args, select_rev_list_commits};
use crate::core::rev_parse::{peel_to_type, resolve_rev};
use crate::objects::{RangeDiffOptions, RangePatch};

/// git's usage text for `range-diff`, printed for `-h` and after usage errors (exit 129).
pub const RANGE_DIFF_USAGE: &str = "usage: rgit range-diff [<options>] <old-base>..<old-tip> <new-base>..<new-tip>
   or: rgit range-diff [<options>] <old-tip>...<new-tip>
   or: rgit range-diff [<options>] <base> <old-tip> <new-tip>

    --creation-factor <n>
                          percentage by which creation is weighted
    --no-dual-color       use simple diff colors
    --left-only           only emit output related to the first range
    --right-only          only emit output related to the second range
    -s, --no-patch        do not show the patch comparison
";

/// A cost no assignment should ever pick (git's `COST_MAX`).
const COST_MAX: i64 = 1 << 16;

/// Parses `range-diff` arguments like git: the options, then either two ranges, one
/// symmetric range `A...B` (meaning `B..A` against `A..B`), or `<base> <old-tip>
/// <new-tip>`. Errors starting with `fatal:` mean exit 128; the rest mean exit 129 (a
/// leading `usage-error: ` marker is stripped before printing).
// Used by `range-diff`.
pub fn parse_range_diff_args(args: &[String]) -> anyhow::Result<RangeDiffOptions> {
    let mut opts = RangeDiffOptions { creation_factor: 60, patch: true, ..Default::default() };
    let mut positional: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        let number = |value: &str| value.parse::<i64>().map_err(|_| anyhow::anyhow!("error: option `creation-factor' expects a numerical value\n"));
        match arg {
            "--" => {
                positional.extend(args[i..].iter());
                break;
            }
            "--creation-factor" => {
                opts.creation_factor = number(args.get(i).map(String::as_str).ok_or_else(|| anyhow::anyhow!("error: option `creation-factor' requires a value\n"))?)?;
                i += 1;
            }
            "--no-dual-color" | "--dual-color" | "--no-color" | "--color" | "--no-notes" => {}
            "--left-only" => opts.left_only = true,
            "--right-only" => opts.right_only = true,
            "-s" | "--no-patch" => opts.patch = false,
            "-p" | "--patch" => opts.patch = true,
            _ if arg.starts_with("--creation-factor=") => opts.creation_factor = number(&arg["--creation-factor=".len()..])?,
            _ if arg.starts_with("--color=") => {}
            _ if arg.starts_with("--") => anyhow::bail!("error: unknown option `{}'\n{}", &arg[2..], RANGE_DIFF_USAGE),
            _ if arg.starts_with('-') && arg.len() > 1 => anyhow::bail!("error: unknown switch `{}'\n{}", &arg[1..2], RANGE_DIFF_USAGE),
            _ => positional.push(&args[i - 1]),
        }
    }
    if opts.left_only && opts.right_only {
        anyhow::bail!("fatal: options '--left-only' and '--right-only' cannot be used together");
    }

    let need_ranges = format!("usage-error: fatal: need two commit ranges\n\n{}", RANGE_DIFF_USAGE);
    let is_commit = |name: &str| resolve_rev(name).and_then(|hash| peel_to_type(hash, "commit")).is_ok();
    match positional.as_slice() {
        [base, old_tip, new_tip] => {
            for name in [base, old_tip, new_tip] {
                if !is_commit(name) {
                    anyhow::bail!("{}", need_ranges);
                }
            }
            opts.range1 = format!("{}..{}", base, old_tip);
            opts.range2 = format!("{}..{}", base, new_tip);
        }
        [range1, range2] => {
            for range in [range1, range2] {
                if !range.contains("..") {
                    anyhow::bail!("{}", need_ranges);
                }
            }
            opts.range1 = (*range1).clone();
            opts.range2 = (*range2).clone();
        }
        [symmetric] => {
            let Some((a, b)) = symmetric.split_once("...") else { anyhow::bail!("{}", need_ranges) };
            let (a, b) = (if a.is_empty() { "HEAD" } else { a }, if b.is_empty() { "HEAD" } else { b });
            opts.range1 = format!("{}..{}", b, a);
            opts.range2 = format!("{}..{}", a, b);
        }
        _ => anyhow::bail!("{}", need_ranges),
    }
    Ok(opts)
}

/// Port of git's `read_patches`: lists the non-merge commits of `range` oldest first and
/// renders each one as the normalized text `range-diff` compares: a ` ## Metadata ##`
/// block with the author, the ` ## Commit message ##`, then for every changed file a
/// ` ## <path> ##` header (with `(new)`, `(deleted)` or the mode change) and its hunks,
/// whose `@@` lines keep only the file name and function context, never line numbers.
// Used by `range-diff`.
pub fn read_range_patches(range: &str) -> anyhow::Result<Vec<RangePatch>> {
    let mut revs = parse_revision_args(&[range.to_string()])?;
    revs.max_parents = Some(1);
    revs.date_order = true;
    let (mut selected, _) = select_rev_list_commits(&revs)?;
    selected.reverse();

    let mut patches = Vec::new();
    for (index, walked) in selected.iter().enumerate() {
        let commit = &walked.commit;
        let mut buf = format!(" ## Metadata ##\nAuthor: {} <{}>\n\n ## Commit message ##\n", commit.author.name.trim_end(), commit.author.email);
        for line in commit.message.trim_end_matches('\n').split('\n') {
            buf.push_str(format!("    {}", line).trim_end());
            buf.push('\n');
        }

        let mut diff_offset = 0;
        let mut diffsize = 0;
        for diff in diff_commit_against_parent(commit)? {
            let path = &diff.stat.name;
            let header: Vec<&str> = diff.text.lines().take_while(|line| !line.starts_with("@@ ")).collect();
            let mode_of = |prefix: &str| header.iter().find_map(|line| line.strip_prefix(prefix)).map(str::to_string);
            let (is_new, is_deleted) = (mode_of("new file mode ").is_some(), mode_of("deleted file mode ").is_some());

            buf.push('\n');
            if diff_offset == 0 {
                diff_offset = buf.len();
            }
            buf.push_str(" ## ");
            buf.push_str(path);
            if is_new {
                buf.push_str(" (new)");
            } else if is_deleted {
                buf.push_str(" (deleted)");
            }
            if let (Some(old_mode), Some(new_mode)) = (mode_of("old mode "), mode_of("new mode ")) {
                if old_mode != new_mode {
                    buf.push_str(&format!(" (mode change {} => {})", old_mode, new_mode));
                }
            }
            buf.push_str(" ##\n");
            diffsize += 1;

            if header.iter().any(|line| line.starts_with("GIT binary patch") || line.starts_with("Binary files ")) {
                let (old_name, new_name) = (if is_new { "/dev/null" } else { path.as_str() }, if is_deleted { "/dev/null" } else { path.as_str() });
                buf.push_str(&format!(" Binary files {} and {} differ\n", old_name, new_name));
                diffsize += 1;
                continue;
            }
            for line in diff.text.lines().skip(header.len()) {
                if let Some(rest) = line.strip_prefix("@@ ") {
                    buf.push_str("@@");
                    if let Some(at) = rest.find("@@") {
                        let context = &rest[at + 2..];
                        if !context.is_empty() {
                            buf.push_str(&format!(" {}:", path));
                        }
                        buf.push_str(context);
                    }
                } else if line.is_empty() {
                    continue;
                } else if line.starts_with(['+', '-', ' ']) {
                    buf.push_str(line);
                } else {
                    buf.push(' ');
                    buf.push_str(line);
                }
                buf.push('\n');
                diffsize += 1;
            }
        }
        if diff_offset == 0 {
            diff_offset = buf.len();
        }
        patches.push(RangePatch { index, hash: commit.hash.clone(), subject: split_commit_message(&commit.message).0, patch: buf, diff_offset, diffsize, matching: None, shown: false });
    }
    Ok(patches)
}

/// The changed runs between two texts and the hunks they form with three lines of context
/// (runs at most six lines apart share a hunk), as `(first run, last run, old start, old
/// end, new start, new end)`.
// Used by `range-diff` (internal to this file's helpers).
fn hunks_of(old: &[String], new: &[String], indent_heuristic: bool) -> (Vec<(usize, usize, usize, usize)>, Vec<bool>, Vec<bool>, Vec<(usize, usize, usize, usize)>) {
    let (changes, rchg_old, rchg_new) = compacted_changes(old, new, indent_heuristic);
    let mut hunks = Vec::new();
    let mut c = 0;
    while c < changes.len() {
        let mut last = c;
        while last + 1 < changes.len() && changes[last + 1].0 - (changes[last].0 + changes[last].1) <= 6 {
            last += 1;
        }
        let (first_i, _, first_j, _) = changes[c];
        let (last_i, last_ci, last_j, last_cj) = changes[last];
        let trailing = 3.min(old.len() - (last_i + last_ci)).min(new.len() - (last_j + last_cj));
        hunks.push((first_i.saturating_sub(3), last_i + last_ci + trailing, first_j.saturating_sub(3), last_j + last_cj + trailing));
        c = last + 1;
    }
    (changes, rchg_old, rchg_new, hunks)
}

/// Splits text into lines that keep their newline.
// Used by `range-diff` (internal to this file's helpers).
fn text_lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(str::to_string).collect()
}

/// git's `diffsize`: how many lines a unified diff between two patch bodies has (hunk
/// headers included); the smaller, the more alike the patches are.
// Used by `match_range_patches`.
fn diff_size(a: &str, b: &str) -> i64 {
    let (old, new) = (text_lines(a), text_lines(b));
    let (_, _, rchg_new, hunks) = hunks_of(&old, &new, false);
    hunks.iter().map(|(s1, e1, s2, e2)| 1 + (e1 - s1) + rchg_new[*s2..*e2].iter().filter(|&&changed| changed).count()).sum::<usize>() as i64
}

/// Port of git's `compute_assignment` (the Jonker-Volgenant algorithm): finds the
/// assignment of columns to rows with the lowest total cost. `cost[column + n * row]` is
/// the cost of pairing them; returns `(column2row, row2column)`.
// Used by `match_range_patches`.
fn compute_assignment(n: usize, cost: &[i64]) -> (Vec<i64>, Vec<i64>) {
    let c = |column: usize, row: usize| cost[column + n * row];
    if n < 2 {
        return (vec![0; n], vec![0; n]);
    }
    let mut column2row = vec![-1i64; n];
    let mut row2column = vec![-1i64; n];
    let mut v = vec![0i64; n];

    // Column reduction.
    for j in (0..n).rev() {
        let mut i1 = 0;
        for i in 1..n {
            if c(j, i1) > c(j, i) {
                i1 = i;
            }
        }
        v[j] = c(j, i1);
        if row2column[i1] == -1 {
            row2column[i1] = j as i64;
            column2row[j] = i1 as i64;
        } else {
            if row2column[i1] >= 0 {
                row2column[i1] = -2 - row2column[i1];
            }
            column2row[j] = -1;
        }
    }

    // Reduction transfer.
    let mut free_row: Vec<usize> = Vec::new();
    for i in 0..n {
        let j1 = row2column[i];
        if j1 == -1 {
            free_row.push(i);
        } else if j1 < -1 {
            row2column[i] = -2 - j1;
        } else {
            let j1 = j1 as usize;
            let other = usize::from(j1 == 0);
            let mut min = c(other, i) - v[other];
            for j in 1..n {
                if j != j1 && min > c(j, i) - v[j] {
                    min = c(j, i) - v[j];
                }
            }
            v[j1] -= min;
        }
    }
    if free_row.is_empty() {
        return (column2row, row2column);
    }

    // Augmenting row reduction.
    for _phase in 0..2 {
        let saved: Vec<usize> = std::mem::take(&mut free_row);
        let mut pending = saved.clone();
        let mut k = 0;
        while k < pending.len() {
            let i = pending[k];
            k += 1;
            let mut j1 = 0usize;
            let mut u1 = c(j1, i) - v[j1];
            let mut j2: i64 = -1;
            let mut u2 = i64::MAX;
            for j in 1..n {
                let cj = c(j, i) - v[j];
                if u2 > cj {
                    if u1 < cj {
                        u2 = cj;
                        j2 = j as i64;
                    } else {
                        u2 = u1;
                        u1 = cj;
                        j2 = j1 as i64;
                        j1 = j;
                    }
                }
            }
            if j2 < 0 {
                j2 = j1 as i64;
                u2 = u1;
            }
            let mut i0 = column2row[j1];
            if u1 < u2 {
                v[j1] -= u2 - u1;
            } else if i0 >= 0 {
                j1 = j2 as usize;
                i0 = column2row[j1];
            }
            if i0 >= 0 {
                if u1 < u2 {
                    k -= 1;
                    pending[k] = i0 as usize;
                } else {
                    free_row.push(i0 as usize);
                }
            }
            row2column[i] = j1 as i64;
            column2row[j1] = i as i64;
        }
    }

    // Augmentation.
    for &i1 in &free_row.clone() {
        let mut d: Vec<i64> = (0..n).map(|j| c(j, i1) - v[j]).collect();
        let mut pred = vec![i1; n];
        let mut col: Vec<usize> = (0..n).collect();
        let (mut low, mut up) = (0usize, 0usize);
        let mut last;
        let mut min;
        let end_column: usize;
        'search: loop {
            last = low;
            min = d[col[up]];
            up += 1;
            for k in up..n {
                let j = col[k];
                let cj = d[j];
                if cj <= min {
                    if cj < min {
                        up = low;
                        min = cj;
                    }
                    col[k] = col[up];
                    col[up] = j;
                    up += 1;
                }
            }
            if let Some(k) = (low..up).find(|&k| column2row[col[k]] == -1) {
                end_column = col[k];
                break 'search;
            }
            // Scan a row.
            loop {
                let j1 = col[low];
                low += 1;
                let i = column2row[j1] as usize;
                let u1 = c(j1, i) - v[j1] - min;
                for k in up..n {
                    let j = col[k];
                    let cj = c(j, i) - v[j] - u1;
                    if cj < d[j] {
                        d[j] = cj;
                        pred[j] = i;
                        if cj == min {
                            if column2row[j] == -1 {
                                end_column = j;
                                break 'search;
                            }
                            col[k] = col[up];
                            col[up] = j;
                            up += 1;
                        }
                    }
                }
                if low == up {
                    break;
                }
            }
        }
        for &j1 in col.iter().take(last) {
            v[j1] += d[j1] - min;
        }
        let mut j = end_column as i64;
        loop {
            let i = pred[j as usize];
            column2row[j as usize] = i as i64;
            std::mem::swap(&mut j, &mut row2column[i]);
            if i == i1 {
                break;
            }
        }
    }
    (column2row, row2column)
}

/// Ports of git's `find_exact_matches` and `get_correspondences`: pairs up the commits of
/// the two ranges. Commits with identical diffs are matched first; the rest are matched by
/// minimum total "diff of diffs" size, where leaving a commit unmatched costs
/// `creation_factor` percent of its own diff size.
// Used by `range-diff`.
pub fn match_range_patches(a: &mut [RangePatch], b: &mut [RangePatch], creation_factor: i64) {
    let mut by_diff: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, patch) in a.iter().enumerate() {
        by_diff.entry(&patch.patch[patch.diff_offset..]).or_default().push(i);
    }
    let mut exact: Vec<(usize, usize)> = Vec::new();
    for (j, patch) in b.iter().enumerate() {
        if let Some(candidates) = by_diff.get_mut(&patch.patch[patch.diff_offset..]) {
            if !candidates.is_empty() {
                exact.push((candidates.remove(0), j));
            }
        }
    }
    for (i, j) in exact {
        a[i].matching = Some(j);
        b[j].matching = Some(i);
    }

    let n = a.len() + b.len();
    let mut cost = vec![0i64; n * n];
    for (i, pa) in a.iter().enumerate() {
        for (j, pb) in b.iter().enumerate() {
            cost[i + n * j] = if pa.matching == Some(j) {
                0
            } else if pa.matching.is_none() && pb.matching.is_none() {
                diff_size(&pa.patch[pa.diff_offset..], &pb.patch[pb.diff_offset..])
            } else {
                COST_MAX
            };
        }
        let unmatched = if pa.matching.is_none() { pa.diffsize as i64 * creation_factor / 100 } else { COST_MAX };
        for j in b.len()..n {
            cost[i + n * j] = unmatched;
        }
    }
    for (j, pb) in b.iter().enumerate() {
        let unmatched = if pb.matching.is_none() { pb.diffsize as i64 * creation_factor / 100 } else { COST_MAX };
        for i in a.len()..n {
            cost[i + n * j] = unmatched;
        }
    }

    let (a2b, _) = compute_assignment(n, &cost);
    for i in 0..a.len() {
        if a2b[i] >= 0 && (a2b[i] as usize) < b.len() {
            a[i].matching = Some(a2b[i] as usize);
            b[a2b[i] as usize].matching = Some(i);
        }
    }
}

/// The section a hunk of the patch comparison belongs to, like git's `section_headers`
/// funcname pattern: the text of ` ## <section> ##` or of an inner `@@ <context>` line.
// Used by `patch_comparison`.
fn section_of(line: &str) -> Option<String> {
    let line = line.trim_end_matches('\n');
    if let Some(inner) = line.strip_prefix(" ## ").and_then(|rest| rest.strip_suffix(" ##")) {
        return Some(inner.to_string());
    }
    let rest = line.strip_prefix("@@ ").or_else(|| line.get(1..).filter(|_| line.is_char_boundary(1)).and_then(|tail| tail.strip_prefix("@@ ")))?;
    Some(rest.to_string())
}

/// Port of git's `patch_diff`: the diff between two normalized patches, indented four
/// spaces, with hunk headers reduced to `@@ <section>` (the commit-message or file section
/// the hunk is in). Lines read as a diff of diffs: the first column says what changed
/// between the two versions, the second is the patch's own `+`/`-`/space.
// Used by `range-diff` for pairs marked `!`.
pub fn patch_comparison(a: &str, b: &str) -> String {
    let (old, new) = (text_lines(a), text_lines(b));
    let (_, rchg_old, rchg_new, hunks) = hunks_of(&old, &new, true);
    let mut out = String::new();
    let mut section = String::new();
    let mut search_limit: i64 = -1;
    for (s1, e1, s2, e2) in hunks {
        let start = s1 as i64 - 1;
        let mut l = start;
        while l != search_limit && l >= 0 {
            if let Some(found) = section_of(&old[l as usize]) {
                section = found;
                break;
            }
            l -= 1;
        }
        search_limit = start;

        out.push_str("    @@");
        if !section.is_empty() {
            out.push(' ');
            out.push_str(&section);
        }
        out.push('\n');
        let (mut i, mut j) = (s1, s2);
        while i < e1 || j < e2 {
            let (marker, line) = if i < e1 && rchg_old[i] {
                i += 1;
                ('-', &old[i - 1])
            } else if j < e2 && rchg_new[j] {
                j += 1;
                ('+', &new[j - 1])
            } else {
                i += 1;
                j += 1;
                (' ', &new[j - 1])
            };
            out.push_str(&format!("    {}{}", marker, line));
            if !line.ends_with('\n') {
                out.push('\n');
            }
        }
    }
    out
}

/// Port of git's `output`: one line per commit pair in the order of the second range,
/// `<n>:  <hash> <status> <m>:  <hash> <subject>`, where the status is `=` (same patch),
/// `!` (changed, followed by the patch comparison), `<` (only in the first range) or `>`
/// (only in the second). Commits dropped from the first range appear once everything
/// before them has been shown.
// Used by `range-diff`.
pub fn format_range_diff(a: &mut [RangePatch], b: &[RangePatch], opts: &RangeDiffOptions) -> String {
    let width = (1 + a.len().max(b.len())).to_string().len();
    let dashes = "-".repeat(7);
    let side = |patch: Option<&RangePatch>| match patch {
        Some(p) => format!("{:>w$}:  {}", p.index + 1, &p.hash[..7], w = width),
        None => format!("{:>w$}:  {}", "-", dashes, w = width),
    };
    let header = |pa: Option<&RangePatch>, pb: Option<&RangePatch>| {
        let status = match (pa, pb) {
            (Some(_), None) => '<',
            (None, _) => '>',
            (Some(x), Some(y)) if x.patch != y.patch => '!',
            _ => '=',
        };
        let subject = pa.or(pb).map(|p| p.subject.as_str()).unwrap_or("");
        format!("{} {} {} {}\n", side(pa), status, side(pb), subject)
    };

    let mut out = String::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        while i < a.len() && a[i].shown {
            i += 1;
        }
        if i < a.len() && a[i].matching.is_none() {
            if !opts.right_only {
                out.push_str(&header(Some(&a[i]), None));
            }
            i += 1;
            continue;
        }
        while j < b.len() && b[j].matching.is_none() {
            if !opts.left_only {
                out.push_str(&header(None, Some(&b[j])));
            }
            j += 1;
        }
        if j < b.len() {
            let matched = b[j].matching.expect("checked above");
            out.push_str(&header(Some(&a[matched]), Some(&b[j])));
            if opts.patch {
                out.push_str(&patch_comparison(&a[matched].patch, &b[j].patch));
            }
            a[matched].shown = true;
            j += 1;
        }
    }
    out
}
