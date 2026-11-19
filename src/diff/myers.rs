use crate::objects::DiffOp;

/// Generates an edit script comparing two lists of text lines using the Myers diff algorithm
// for shortest path in a 2d grid.
// Used by the `diff` command to generate line-by-line differences between file versions.
pub fn myers_diff(old_lines: &[&str], new_lines: &[&str]) -> Vec<DiffOp> {
    let n = old_lines.len();
    let m = new_lines.len();
    let max = n + m;

    if max == 0 {
        return Vec::new();
    }

    let mut v = vec![0isize; 2 * max + 1];
    let offset = max as isize;

    let mut trace: Vec<Vec<isize>> = Vec::new();

    for d in 0..=(max as isize) {
        trace.push(v.clone());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d || (k != d && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize]) {
                v[(k + 1 + offset) as usize]
            } else {
                v[(k - 1 + offset) as usize] + 1
            };
            let mut y = x - k;

            while (x as usize) < n && (y as usize) < m && old_lines[x as usize] == new_lines[y as usize] {
                x += 1;
                y += 1;
            }

            v[(k + offset) as usize] = x;

            if x as usize >= n && y as usize >= m {
                return backtrack_myers(&trace, old_lines, new_lines, n, m);
            }

            k += 2;
        }
    }

    Vec::new()
}

fn backtrack_myers(trace: &[Vec<isize>], old_lines: &[&str], new_lines: &[&str], n: usize, m: usize) -> Vec<DiffOp> {
    let mut x = n as isize;
    let mut y = m as isize;
    let max = n + m;
    let offset = max as isize;

    let mut ops = Vec::new();

    for d in (0..trace.len()).rev() {
        let v = &trace[d];
        let k = x - y;

        let prev_k = if k == -(d as isize) || (k != (d as isize) && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize]) {
            k + 1
        } else {
            k - 1
        };

        let prev_x = v[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            x -= 1;
            y -= 1;
            ops.push(DiffOp::Keep(old_lines[x as usize].to_string()));
        }

        if d > 0 {
            if x == prev_x {
                y -= 1;
                ops.push(DiffOp::Insert(new_lines[y as usize].to_string()));
            } else if y == prev_y {
                x -= 1;
                ops.push(DiffOp::Delete(old_lines[x as usize].to_string()));
            }
        }
    }

    ops.reverse();
    ops
}

/// Formats a list of line diff operations into unified diff format (with headers and line hunks).
// Used by `diff` command.
pub fn format_diff_output(path: &str, old_lines: &[&str], new_lines: &[&str], old_label: &str, new_label: &str,) -> String {
    let ops = myers_diff(old_lines, new_lines);

    let mut has_changes = false;
    for op in &ops {
        if matches!(op, DiffOp::Delete(_) | DiffOp::Insert(_)) {
            has_changes = true;
            break;
        }
    }

    if !has_changes {
        return String::new();
    }

    let mut output = String::new();
    output.push_str(&format!("diff --git a/{} b/{}\n", path, path));
    output.push_str(&format!("--- {}\n", old_label));
    output.push_str(&format!("+++ {}\n", new_label));

    let context_size = 3;
    let mut i = 0;

    while i < ops.len() {
        if matches!(ops[i], DiffOp::Keep(_)) {
            i += 1;
            continue;
        }

        let hunk_start = i.saturating_sub(context_size);
        let mut hunk_end = i;

        let mut lookahead = i;
        while lookahead < ops.len() {
            if matches!(ops[lookahead], DiffOp::Delete(_) | DiffOp::Insert(_)) {
                hunk_end = (lookahead + context_size + 1).min(ops.len());
            } else {
                let next_change = (lookahead..ops.len()).find(|&j| matches!(ops[j], DiffOp::Delete(_) | DiffOp::Insert(_)));
                if let Some(nc) = next_change {
                    if nc - lookahead <= 2 * context_size {
                        lookahead = nc;
                        continue;
                    }
                }
                break;
            }
            lookahead += 1;
        }

        let mut old_line_num = 1;
        let mut new_line_num = 1;
        for op in &ops[..hunk_start] {
            match op {
                DiffOp::Keep(_) => {
                    old_line_num += 1;
                    new_line_num += 1;
                }
                DiffOp::Delete(_) => old_line_num += 1,
                DiffOp::Insert(_) => new_line_num += 1,
            }
        }

        let mut old_count = 0;
        let mut new_count = 0;
        for op in &ops[hunk_start..hunk_end] {
            match op {
                DiffOp::Keep(_) => {
                    old_count += 1;
                    new_count += 1;
                }
                DiffOp::Delete(_) => old_count += 1,
                DiffOp::Insert(_) => new_count += 1,
            }
        }

        output.push_str(&format!("@@ -{},{} +{},{} @@\n", old_line_num, old_count, new_line_num, new_count));

        for op in &ops[hunk_start..hunk_end] {
            match op {
                DiffOp::Keep(line) => output.push_str(&format!(" {}\n", line)),
                DiffOp::Delete(line) => output.push_str(&format!("-{}\n", line)),
                DiffOp::Insert(line) => output.push_str(&format!("+{}\n", line)),
            }
        }

        i = hunk_end;
    }

    output
}