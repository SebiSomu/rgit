//! A port of git's `graph.c`: the ASCII commit graph `log --graph` draws to the left of each
//! commit. The drawing is a state machine fed one commit at a time (`graph_update`); every
//! output line (commit row, message line, diff line, separator) asks it for the next prefix.

use crate::objects::{GitGraph, GraphState};

/// Creates an empty graph drawing, ready for its first `graph_update`.
// Used by `log --graph`.
pub fn graph_new() -> GitGraph {
    GitGraph { merge_layout: -1, ..Default::default() }
}

/// Moves the drawing on to the next commit (git's `graph_update`): records its interesting
/// parents (the caller filters them: parents that are shown, only the first with
/// `--first-parent`), computes the branch lines for the rows after it, and picks the first
/// state: `Skip` when the previous commit never finished its lines, `PreCommit` when an octopus
/// merge needs room, otherwise `Commit`.
// Used by `log --graph`.
pub fn graph_update(g: &mut GitGraph, commit: &str, mark: &str, parents: Vec<String>) {
    g.commit = Some(commit.to_string());
    g.mark = mark.to_string();
    g.parents = parents;
    g.prev_commit_index = g.commit_index;
    graph_update_columns(g);
    g.expansion_row = 0;
    // No line was printed for the old state, so prev_state stays as it is.
    g.state = if g.state != GraphState::Padding {
        GraphState::Skip
    } else if graph_needs_pre_commit_line(g) {
        GraphState::PreCommit
    } else {
        GraphState::Commit
    };
}

/// Whether every line for the current commit has been printed (git's
/// `graph_is_commit_finished`).
// Used by `log --graph`.
pub fn graph_is_commit_finished(g: &GitGraph) -> bool {
    g.state == GraphState::Padding
}

/// How many interesting parents the current commit has.
// Used by graph layout helpers.
fn num_parents(g: &GitGraph) -> i32 {
    g.parents.len() as i32
}

/// How many `-` pairs an octopus merge's commit row needs (git's `graph_num_dashed_parents`).
// Used by `draw_octopus_merge` and `num_expansion_rows`.
fn num_dashed_parents(g: &GitGraph) -> i32 {
    num_parents(g) + g.merge_layout - 3
}

/// Rows needed before an octopus merge to make room for its parent lines.
// Used by `graph_needs_pre_commit_line`.
fn num_expansion_rows(g: &GitGraph) -> i32 {
    num_dashed_parents(g) * 2
}

/// Whether an octopus merge still needs a row widening the lines to its right.
// Used by `graph_update` and the pre-commit/skip rows.
fn graph_needs_pre_commit_line(g: &GitGraph) -> bool {
    num_parents(g) >= 3 && g.commit_index < g.columns.len() as i32 - 1 && g.expansion_row < num_expansion_rows(g)
}

/// Moves to the next state, remembering the one whose line was just printed.
// Used by every graph row writer.
fn update_state(g: &mut GitGraph, s: GraphState) {
    g.prev_state = g.state;
    g.state = s;
}

/// Whether a branch line leads to the commit being shown.
// Used by the graph row writers.
fn is_current(g: &GitGraph, commit: &str) -> bool {
    g.commit.as_deref() == Some(commit)
}

/// git's `graph_update_columns`: the branch lines after the current commit (its parents take
/// its place; every other line stays) and where each screen column ends up.
// Used by `graph_update`.
fn graph_update_columns(g: &mut GitGraph) {
    std::mem::swap(&mut g.columns, &mut g.new_columns);
    g.new_columns.clear();

    let max_new_columns = g.columns.len() + g.parents.len();
    g.mapping_size = 2 * max_new_columns;
    if g.mapping.len() < g.mapping_size {
        g.mapping.resize(g.mapping_size, -1);
    }
    if g.old_mapping.len() < g.mapping_size {
        g.old_mapping.resize(g.mapping_size, -1);
    }
    for slot in g.mapping.iter_mut().take(g.mapping_size) {
        *slot = -1;
    }

    g.width = 0;
    g.prev_edges_added = g.edges_added;
    g.edges_added = 0;

    let num_columns = g.columns.len();
    let current = g.commit.clone().unwrap_or_default();
    let mut seen_this = false;
    for i in 0..=num_columns {
        let col_commit = if i == num_columns {
            if seen_this {
                break;
            }
            current.clone()
        } else {
            g.columns[i].clone()
        };
        if col_commit == current {
            seen_this = true;
            g.commit_index = i as i32;
            g.merge_layout = -1;
            for parent in g.parents.clone() {
                insert_into_new_columns(g, &parent, i as i32);
            }
            // The commit itself always takes up at least two screen columns.
            if g.parents.is_empty() {
                g.width += 2;
            }
        } else {
            insert_into_new_columns(g, &col_commit, -1);
        }
    }

    while g.mapping_size > 1 && g.mapping[g.mapping_size - 1] < 0 {
        g.mapping_size -= 1;
    }
}

/// git's `graph_insert_into_new_columns`: adds a branch line (once per commit) and records the
/// screen column that leads to it, choosing the skew of a merge from where its first parent is.
// Used by `graph_update_columns`.
fn insert_into_new_columns(g: &mut GitGraph, commit: &str, idx: i32) {
    let i = match g.new_columns.iter().position(|c| c == commit) {
        Some(i) => i as i32,
        None => {
            g.new_columns.push(commit.to_string());
            g.new_columns.len() as i32 - 1
        }
    };

    let mapping_idx;
    if num_parents(g) > 1 && idx > -1 && g.merge_layout == -1 {
        // The first parent of a merge decides the layout of the merge line.
        let dist = idx - i;
        let shift = if dist > 1 { 2 * dist - 3 } else { 1 };
        g.merge_layout = if dist > 0 { 0 } else { 1 };
        g.edges_added = num_parents(g) + g.merge_layout - 2;
        mapping_idx = g.width + (g.merge_layout - 1) * shift;
        g.width += 2 * g.merge_layout;
    } else if g.edges_added > 0 && g.width >= 2 && i == g.mapping[(g.width - 2) as usize] {
        // A merge added lines, but this one joins the last existing line straight away.
        mapping_idx = g.width - 2;
        g.edges_added = -1;
    } else {
        mapping_idx = g.width;
        g.width += 2;
    }
    let mapping_idx = mapping_idx.max(0) as usize;
    if g.mapping.len() <= mapping_idx {
        g.mapping.resize(mapping_idx + 1, -1);
        g.old_mapping.resize(mapping_idx + 1, -1);
    }
    g.mapping[mapping_idx] = i;
}

/// Each branch line sits at its target, or one column right of it (a `/` gets it there).
// Used by the commit, post-merge and collapsing rows.
fn graph_is_mapping_correct(g: &GitGraph) -> bool {
    (0..g.mapping_size).all(|i| {
        let target = g.mapping[i];
        target < 0 || target == (i / 2) as i32
    })
}

/// Produces the next line of the drawing, padded to the commit's width, and whether it was the
/// commit row (git's `graph_next_line`).
// Used by `log --graph`.
pub fn graph_next_line(g: &mut GitGraph) -> (String, bool) {
    if g.commit.is_none() {
        return (String::new(), false);
    }
    let mut line = String::new();
    let mut shown_commit_line = false;
    match g.state {
        GraphState::Padding => output_padding_line(g, &mut line),
        GraphState::Skip => output_skip_line(g, &mut line),
        GraphState::PreCommit => output_pre_commit_line(g, &mut line),
        GraphState::Commit => {
            output_commit_line(g, &mut line);
            shown_commit_line = true;
        }
        GraphState::PostMerge => output_post_merge_line(g, &mut line),
        GraphState::Collapsing => output_collapsing_line(g, &mut line),
    }
    pad_horizontally(g, &mut line);
    (line, shown_commit_line)
}

/// Pads a row with spaces to the width of the current commit, so text to its right lines up.
// Used by `graph_next_line` and `graph_padding_line`.
fn pad_horizontally(g: &GitGraph, line: &mut String) {
    let width = line.chars().count() as i32;
    if width < g.width {
        line.push_str(&" ".repeat((g.width - width) as usize));
    }
}

/// Writes a row that keeps every branch line as it is.
// Used by `graph_next_line`.
fn output_padding_line(g: &GitGraph, line: &mut String) {
    for _ in &g.new_columns {
        line.push_str("| ");
    }
}

/// Writes `...` where the previous commit never finished its rows (git skipped showing it).
// Used by `graph_next_line`.
fn output_skip_line(g: &mut GitGraph, line: &mut String) {
    line.push_str("...");
    let next = if graph_needs_pre_commit_line(g) { GraphState::PreCommit } else { GraphState::Commit };
    update_state(g, next);
}

/// Writes one of the rows that widen the lines right of an octopus merge.
// Used by `graph_next_line`.
fn output_pre_commit_line(g: &mut GitGraph, line: &mut String) {
    let mut seen_this = false;
    for i in 0..g.columns.len() {
        if is_current(g, &g.columns[i]) {
            seen_this = true;
            line.push('|');
            line.push_str(&" ".repeat(g.expansion_row.max(0) as usize));
        } else if seen_this && g.expansion_row == 0 {
            // Lines right of a merge printed as `\` on the row before stay `\` here.
            if g.prev_state == GraphState::PostMerge && g.prev_commit_index < i as i32 {
                line.push('\\');
            } else {
                line.push('|');
            }
        } else if seen_this && g.expansion_row > 0 {
            line.push('\\');
        } else {
            line.push('|');
        }
        line.push(' ');
    }
    g.expansion_row += 1;
    if !graph_needs_pre_commit_line(g) {
        update_state(g, GraphState::Commit);
    }
}

/// Writes the `-.`/`---.` run after an octopus merge's mark.
// Used by `output_commit_line`.
fn draw_octopus_merge(g: &GitGraph, line: &mut String) {
    let dashed = num_dashed_parents(g);
    for i in 0..dashed {
        line.push('-');
        line.push(if i == dashed - 1 { '.' } else { '-' });
    }
}

/// Writes the commit's own row (its mark among the other lines) and picks the next state.
// Used by `graph_next_line`.
fn output_commit_line(g: &mut GitGraph, line: &mut String) {
    let num_columns = g.columns.len();
    let mut seen_this = false;
    for i in 0..=num_columns {
        let is_this = if i == num_columns {
            if seen_this {
                break;
            }
            true
        } else {
            is_current(g, &g.columns[i])
        };
        if is_this {
            seen_this = true;
            line.push_str(&g.mark);
            if num_parents(g) > 2 {
                draw_octopus_merge(g, line);
            }
        } else if seen_this && g.edges_added > 1 {
            line.push('\\');
        } else if seen_this && g.edges_added == 1 {
            // A right-skewed two-way merge (or left-skewed three-way): a line that was `\` on
            // the post-merge row of the previous commit stays `\`.
            if g.prev_state == GraphState::PostMerge && g.prev_edges_added > 0 && g.prev_commit_index < i as i32 {
                line.push('\\');
            } else {
                line.push('|');
            }
        } else if g.prev_state == GraphState::Collapsing
            && g.old_mapping.get(2 * i + 1).copied() == Some(i as i32)
            && g.mapping.get(2 * i).copied().unwrap_or(-1) < i as i32
        {
            line.push('/');
        } else {
            line.push('|');
        }
        line.push(' ');
    }

    if num_parents(g) > 1 {
        update_state(g, GraphState::PostMerge);
    } else if graph_is_mapping_correct(g) {
        update_state(g, GraphState::Padding);
    } else {
        update_state(g, GraphState::Collapsing);
    }
}

/// Writes the row that fans a merge out to its parents (`|\\`, `|/|`).
// Used by `graph_next_line`.
fn output_post_merge_line(g: &mut GitGraph, line: &mut String) {
    const MERGE_CHARS: [char; 3] = ['/', '|', '\\'];
    let num_columns = g.columns.len();
    let mut seen_this = false;
    for i in 0..=num_columns {
        let is_this = if i == num_columns {
            if seen_this {
                break;
            }
            true
        } else {
            is_current(g, &g.columns[i])
        };
        if is_this {
            // Fan out to the columns of the parents.
            seen_this = true;
            let mut idx = g.merge_layout.max(0) as usize;
            let n = g.parents.len();
            for j in 0..n {
                line.push(MERGE_CHARS[idx.min(2)]);
                if idx == 2 {
                    if g.edges_added > 0 || j < n - 1 {
                        line.push(' ');
                    }
                } else {
                    idx += 1;
                }
            }
            if g.edges_added == 0 {
                line.push(' ');
            }
        } else if seen_this {
            line.push(if g.edges_added > 0 { '\\' } else { '|' });
            line.push(' ');
        } else {
            line.push('|');
            if g.merge_layout != 0 || i as i32 != g.commit_index - 1 {
                line.push(' ');
            }
        }
    }
    if graph_is_mapping_correct(g) {
        update_state(g, GraphState::Padding);
    } else {
        update_state(g, GraphState::Collapsing);
    }
}

/// Writes a row that moves branch lines left (`/`, `_`) towards their columns.
// Used by `graph_next_line`.
fn output_collapsing_line(g: &mut GitGraph, line: &mut String) {
    let mut used_horizontal = false;
    let mut horizontal_edge: i32 = -1;
    let mut horizontal_edge_target: i32 = -1;

    std::mem::swap(&mut g.mapping, &mut g.old_mapping);
    let size = g.mapping_size;
    for slot in g.mapping.iter_mut().take(size) {
        *slot = -1;
    }

    for i in 0..size {
        let target = g.old_mapping[i];
        if target < 0 {
            continue;
        }
        let t2 = (target * 2) as usize;
        if t2 == i {
            // Already in place.
            g.mapping[i] = target;
        } else if g.mapping[i - 1] < 0 {
            // Nothing to the left: move one column left.
            g.mapping[i - 1] = target;
            if horizontal_edge == -1 {
                horizontal_edge = i as i32;
                horizontal_edge_target = target;
                let mut j = t2 + 3;
                while j + 2 < i {
                    g.mapping[j] = target;
                    j += 2;
                }
            }
        } else if g.mapping[i - 1] == target {
            // Joins the line to the left, which leads to the same commit.
        } else {
            // Crosses the line to the left; the space before it is free.
            if i >= 2 {
                g.mapping[i - 2] = target;
            }
            if horizontal_edge == -1 {
                horizontal_edge_target = target;
                horizontal_edge = i as i32 - 1;
                let mut j = t2 + 3;
                while j + 2 < i {
                    g.mapping[j] = target;
                    j += 2;
                }
            }
        }
    }

    // Keep this row's mapping for the next commit row (it decides where `/` goes).
    let (left, right) = (g.mapping[..size].to_vec(), &mut g.old_mapping);
    right[..size].copy_from_slice(&left);

    if size > 0 && g.mapping[size - 1] < 0 {
        g.mapping_size -= 1;
    }

    for i in 0..g.mapping_size {
        let target = g.mapping[i];
        if target < 0 {
            line.push(' ');
        } else if (target * 2) as usize == i {
            line.push('|');
        } else if target == horizontal_edge_target && i as i32 != horizontal_edge - 1 {
            // Only the first segment of a horizontal edge carries on to the next row.
            if i != (target * 2) as usize + 3 {
                g.mapping[i] = -1;
            }
            used_horizontal = true;
            line.push('_');
        } else {
            if used_horizontal && (i as i32) < horizontal_edge {
                g.mapping[i] = -1;
            }
            line.push('/');
        }
    }

    if graph_is_mapping_correct(g) {
        update_state(g, GraphState::Padding);
    }
}

/// A line that keeps every branch line as it is, for separators and diff output (git's
/// `graph_padding_line`); before the commit row has been printed it is the next line instead.
// Used by `log --graph`.
pub fn graph_padding_line(g: &mut GitGraph) -> String {
    if g.state != GraphState::Commit {
        return graph_next_line(g).0;
    }
    let mut line = String::new();
    for i in 0..g.columns.len() {
        line.push('|');
        if is_current(g, &g.columns[i]) && g.parents.len() > 2 {
            line.push_str(&" ".repeat((g.parents.len() - 2) * 2));
        } else {
            line.push(' ');
        }
    }
    pad_horizontally(g, &mut line);
    g.prev_state = GraphState::Padding;
    line
}

/// The lines before a commit's text (git's `graph_show_commit`): every pending line, each with
/// its newline, then the commit row's prefix with none, so the commit text follows on it.
// Used by `log --graph`.
pub fn graph_show_commit(g: &mut GitGraph) -> String {
    let mut out = String::new();
    if graph_is_commit_finished(g) {
        out.push_str(&graph_padding_line(g));
        return out;
    }
    let mut shown = false;
    while !shown && !graph_is_commit_finished(g) {
        let (line, commit_line) = graph_next_line(g);
        shown = commit_line;
        out.push_str(&line);
        if !shown {
            out.push('\n');
        }
    }
    out
}

/// The lines left for the current commit, joined by newlines, without a final one (git's
/// `graph_show_remainder`).
// Used by `log --graph`.
pub fn graph_show_remainder(g: &mut GitGraph) -> String {
    let mut out = String::new();
    while !graph_is_commit_finished(g) {
        out.push_str(&graph_next_line(g).0);
        if !graph_is_commit_finished(g) {
            out.push('\n');
        }
    }
    out
}

/// Prints a commit's message text with the graph (git's `graph_show_commit_msg`): every line
/// but the first gets the next graph line in front, and lines still owed to this commit (the
/// fan-out after a merge, collapsing lines) follow right after it.
// Used by `log --graph`.
pub fn graph_show_commit_msg(g: &mut GitGraph, text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    loop {
        let (segment, next) = match rest.find('\n') {
            Some(pos) => (&rest[..=pos], Some(&rest[pos + 1..])),
            None => (rest, None),
        };
        out.push_str(segment);
        match next {
            Some(n) if !n.is_empty() => {
                out.push_str(&graph_next_line(g).0);
                rest = n;
            }
            _ => break,
        }
    }
    let newline_terminated = text.ends_with('\n');
    if !graph_is_commit_finished(g) {
        if !newline_terminated {
            out.push('\n');
        }
        out.push_str(&graph_show_remainder(g));
        if newline_terminated {
            out.push('\n');
        }
    }
    out
}

/// Prefixes every line of diff output (and the blank line before it) with a padding line.
// Used by `log --graph`.
pub fn graph_prefix_lines(g: &mut GitGraph, text: &str) -> String {
    let mut out = String::new();
    for line in text.split_inclusive('\n') {
        out.push_str(&graph_padding_line(g));
        out.push_str(line);
    }
    out
}
