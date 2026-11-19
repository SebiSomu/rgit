/// Layout settings of the `column` command, mirroring git's `colopts` bits and
/// `struct column_options`: whether columns are on (`always`/`never`/`auto`), how items are
/// filled in (down the columns, across the rows, or one per line), whether columns shrink to
/// their own widest item (`dense`), and the width / padding / indent / line ending to use.
// Used by: src/plumbing/column/engine.rs
// Used by command/helper: `parse_column_args`, `apply_column_mode`, `render_columns`, and the `column` / `help` commands
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnOptions {
    /// `always` / `never` / `auto` (`auto` = only when standard output is a terminal).
    pub enable: ColumnEnable,
    /// `column`, `row` or `plain`.
    pub layout: ColumnLayout,
    /// `dense` / `nodense`: unequal column widths, to fit more columns.
    pub dense: bool,
    /// `--width=<n>`: total width; 0 means the terminal width minus one.
    pub width: usize,
    /// `--padding=<n>`: spaces between columns (default 1).
    pub padding: usize,
    /// `--indent=<string>`: printed at the start of every line.
    pub indent: String,
    /// `--nl=<string>`: printed at the end of every line (default a newline).
    pub nl: String,
}

/// Whether `column` lays its input out in columns at all.
// Used by: src/plumbing/column/types.rs, src/plumbing/column/engine.rs, src/plumbing/help.rs
// Used by command/helper: `apply_column_mode`, `render_columns`, and the `column` / `help` commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnEnable {
    Never,
    Always,
    Auto,
}

/// How `column` fills its table.
// Used by: src/plumbing/column/types.rs, src/plumbing/column/engine.rs
// Used by command/helper: `apply_column_mode`, `render_columns`, and the `column` / `help` commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnLayout {
    /// Fill the first column top to bottom, then the next one.
    Column,
    /// Fill the first row left to right, then the next one.
    Row,
    /// One item per line.
    Plain,
}
