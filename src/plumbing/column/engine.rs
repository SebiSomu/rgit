use std::io::IsTerminal;
use crate::plumbing::column::types::{ColumnEnable, ColumnLayout, ColumnOptions};

/// Usage text of `column`, printed for `-h` and usage errors.
pub const COLUMN_USAGE: &str = "usage: rgit column [<options>]\n\n    --[no-]command <name> lookup config vars\n    --[no-]mode[=<style>] layout to use\n    --[no-]raw-mode <n>   layout to use\n    --[no-]width <n>      maximum width\n    --[no-]indent <string>\n                          padding space on left border\n    --[no-]nl <string>    padding space on right border\n    --[no-]padding <n>    padding space between columns\n\n";

/// The settings `column` starts from: columns off, column-first layout, padding 1.
// Used by `column` and `help -a`.
pub fn default_column_options() -> ColumnOptions {
    ColumnOptions { enable: ColumnEnable::Never, layout: ColumnLayout::Column, dense: false, width: 0, padding: 1, indent: String::new(), nl: "\n".to_string() }
}

/// Applies a mode string as `--mode=<style>` and `column.ui` take it: tokens separated by
/// spaces or commas out of `always`, `never`, `auto`, `column`, `row`, `plain`, `dense`,
/// `nodense`. Naming a layout without `always`/`never`/`auto` switches columns on.
// Used by `column` (`--mode`, `column.ui`, `column.<command>`).
pub fn apply_column_mode(opts: &mut ColumnOptions, value: &str) -> Result<(), String> {
    let mut enable_set = false;
    let mut layout_set = false;
    for token in value.split([' ', ',']).filter(|t| !t.is_empty()) {
        match token {
            "always" => opts.enable = ColumnEnable::Always,
            "never" => opts.enable = ColumnEnable::Never,
            "auto" => opts.enable = ColumnEnable::Auto,
            "plain" => opts.layout = ColumnLayout::Plain,
            "column" => opts.layout = ColumnLayout::Column,
            "row" => opts.layout = ColumnLayout::Row,
            "dense" => opts.dense = true,
            "nodense" => opts.dense = false,
            other => return Err(format!("error: unsupported option '{}'", other)),
        }
        if matches!(token, "always" | "never" | "auto") {
            enable_set = true;
        } else {
            layout_set = true;
        }
    }
    if layout_set && !enable_set {
        opts.enable = ColumnEnable::Always;
    }
    Ok(())
}

/// Applies git's numeric `--raw-mode=<n>` bits: layout in the low nibble (0 column, 1 row,
/// 15 plain), 0x10 always, 0x20 auto, 0x80 dense.
// Used by `column --raw-mode`.
pub fn apply_column_raw_mode(opts: &mut ColumnOptions, bits: u32) {
    opts.layout = match bits & 0x0F {
        1 => ColumnLayout::Row,
        15 => ColumnLayout::Plain,
        _ => ColumnLayout::Column,
    };
    opts.enable = match bits & 0x30 {
        0x10 => ColumnEnable::Always,
        0x20 => ColumnEnable::Auto,
        _ => ColumnEnable::Never,
    };
    opts.dense = bits & 0x80 != 0;
}

/// Parses the arguments of `column` on top of `opts` (already holding the config defaults).
/// Returns the `--command` name when one is given as the first argument. Errors are the text
/// to print before exiting with the returned status (129 usage, 128 fatal).
// Used by `column`.
pub fn parse_column_args(args: &[String], opts: &mut ColumnOptions) -> Result<(), (String, i32)> {
    let usage = |message: String| (format!("{}\n", message), 129);
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        i += 1;
        if arg == "-h" {
            return Err((COLUMN_USAGE.to_string(), 129));
        }
        let Some(body) = arg.strip_prefix("--") else {
            if arg.starts_with('-') && arg.len() > 1 {
                return Err((format!("error: unknown switch `{}'\n{}", arg.chars().nth(1).unwrap_or('-'), COLUMN_USAGE), 129));
            }
            return Err((COLUMN_USAGE.to_string(), 129));
        };
        let (name, inline) = match body.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (body, None),
        };
        if name == "mode" {
            match inline {
                Some(value) => apply_column_mode(opts, &value).map_err(usage)?,
                None => opts.enable = ColumnEnable::Always,
            }
            continue;
        }
        if !matches!(name, "command" | "raw-mode" | "width" | "indent" | "nl" | "padding") {
            return Err((format!("error: unknown option `{}'\n{}", body, COLUMN_USAGE), 129));
        }
        let value = match inline {
            Some(value) => value,
            None => {
                let next = args.get(i).cloned().ok_or_else(|| usage(format!("error: option `{}' requires a value", name)))?;
                i += 1;
                next
            }
        };
        let number = |value: &str| value.parse::<i64>().map_err(|_| usage(format!("error: option `{}' expects a numerical value", name)));
        match name {
            "command" => {
                // `column` reads `column.<command>` before anything else, so it must come first.
                if i > 2 || !args[0].starts_with("--command") {
                    return Err(("fatal: --command must be the first argument\n".to_string(), 128));
                }
            }
            "raw-mode" => apply_column_raw_mode(opts, number(&value)? as u32),
            "width" => opts.width = number(&value)?.max(0) as usize,
            "padding" => opts.padding = number(&value)?.max(0) as usize,
            "indent" => opts.indent = value,
            _ => opts.nl = value,
        }
    }
    Ok(())
}

/// The `--command=<name>` value when it is the first argument (`--command=x` or `--command x`).
// Used by `column` to pick `column.<name>` from the config.
pub fn column_command_name(args: &[String]) -> Option<String> {
    match args.first().map(String::as_str) {
        Some("--command") => args.get(1).cloned(),
        Some(first) => first.strip_prefix("--command=").map(str::to_string),
        None => None,
    }
}

/// Width of the terminal: `COLUMNS` when set to a positive number, else 80.
// Used by `render_columns` when no `--width` is given.
pub fn terminal_columns() -> usize {
    std::env::var("COLUMNS").ok().and_then(|v| v.trim().parse::<usize>().ok()).filter(|n| *n > 0).unwrap_or(80)
}

/// True for characters that take two terminal cells (CJK, Hangul, fullwidth forms, emoji).
// Used by `display_width`.
fn is_wide_char(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
}

/// Number of terminal cells an item takes: ANSI colour sequences (`ESC [ ... m`) and combining
/// marks count as zero, wide characters as two.
// Used by `render_columns`.
pub fn display_width(text: &str) -> usize {
    let mut width = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        width += match c as u32 {
            0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F => 0,
            _ if is_wide_char(c) => 2,
            _ => 1,
        };
    }
    width
}

/// Position in the list of the cell at column `x`, row `y`.
// Used by `render_columns`.
fn cell_index(layout: ColumnLayout, x: usize, y: usize, cols: usize, rows: usize) -> usize {
    if layout == ColumnLayout::Column { x * rows + y } else { y * cols + x }
}

/// Width of the widest item in each column.
// Used by `render_columns` for dense layouts.
fn column_widths(layout: ColumnLayout, lens: &[usize], cols: usize, rows: usize) -> Vec<usize> {
    (0..cols)
        .map(|x| (0..rows).map(|y| cell_index(layout, x, y, cols, rows)).filter(|i| *i < lens.len()).map(|i| lens[i]).max().unwrap_or(0))
        .collect()
}

/// Lays `items` out like `git column`: one per line when columns are off or the layout is
/// `plain`; otherwise in as many equal columns as fit the width, filled column-first or
/// row-first, and with `dense` in as few rows as still fit when each column is only as wide
/// as its own widest item.
// Used by `column` and `help -a`.
pub fn render_columns(items: &[String], opts: &ColumnOptions) -> String {
    let active = match opts.enable {
        ColumnEnable::Always => true,
        ColumnEnable::Never => false,
        ColumnEnable::Auto => std::io::stdout().is_terminal(),
    };
    let mut out = String::new();
    if !active {
        for item in items {
            out.push_str(item);
            out.push('\n');
        }
        return out;
    }
    if opts.layout == ColumnLayout::Plain {
        for item in items {
            out.push_str(&opts.indent);
            out.push_str(item);
            out.push_str(&opts.nl);
        }
        return out;
    }
    if items.is_empty() {
        return out;
    }

    let total = if opts.width > 0 { opts.width } else { terminal_columns().saturating_sub(1) };
    let indent_width = opts.indent.len();
    let lens: Vec<usize> = items.iter().map(|item| display_width(item)).collect();
    let cell_width = lens.iter().copied().max().unwrap_or(0) + opts.padding;
    let mut cols = if cell_width == 0 { 1 } else { (total.saturating_sub(indent_width) / cell_width).max(1) };
    let mut rows = items.len().div_ceil(cols);

    let mut widths: Option<Vec<usize>> = None;
    if opts.dense {
        while rows > 1 {
            let try_rows = rows - 1;
            let try_cols = items.len().div_ceil(try_rows);
            let try_widths = column_widths(opts.layout, &lens, try_cols, try_rows);
            let needed = indent_width + try_widths.iter().map(|w| w + opts.padding).sum::<usize>();
            if needed > total {
                break;
            }
            rows = try_rows;
            cols = try_cols;
        }
        widths = Some(column_widths(opts.layout, &lens, cols, rows));
    }

    for y in 0..rows {
        for x in 0..cols {
            let i = cell_index(opts.layout, x, y, cols, rows);
            if i >= items.len() {
                break;
            }
            let last_in_line = if opts.layout == ColumnLayout::Column { i + rows >= items.len() } else { x == cols - 1 || i == items.len() - 1 };
            if x == 0 {
                out.push_str(&opts.indent);
            }
            out.push_str(&items[i]);
            if last_in_line {
                out.push_str(&opts.nl);
            } else {
                let column_width = widths.as_ref().map(|w| w[x] + opts.padding).unwrap_or(cell_width);
                out.push_str(&" ".repeat(column_width.saturating_sub(lens[i])));
            }
        }
    }
    out
}

