pub mod cli;
pub mod engine;
pub mod types;

use std::io::{self, Read, Write};
use anyhow::Result;

use crate::plumbing::column::engine::{apply_column_mode, column_command_name, default_column_options, parse_column_args, render_columns};
use crate::core::config::get_config_value;

/// `rgit column [--command=<name>] [--mode[=<style>]] [--raw-mode=<n>] [--width=<n>]
/// [--indent=<string>] [--nl=<string>] [--padding=<n>]`
///
/// Reads lines from standard input and prints them laid out in columns. `--mode` takes
/// `always` / `never` / `auto` (on, off, on when writing to a terminal), a layout — `column`
/// (fill columns first), `row` (fill rows first) or `plain` (one per line) — and `dense` /
/// `nodense` (columns of unequal width, to fit more); giving a layout switches columns on,
/// and without any mode the input is printed unchanged. `--width` is the total width
/// (default: `COLUMNS` or 80, minus one), `--padding` the gap between columns (default 1),
/// `--indent` a prefix and `--nl` the ending of every line. Defaults come from `column.ui`,
/// and from `column.<name>` when `--command=<name>` is the first argument. Works outside a
/// repository. Exit status: 0, 129 for a usage error, 128 when `--command` is not first.
pub fn column(args: Vec<String>) -> Result<()> {
    let mut opts = default_column_options();
    let mut keys = vec!["column.ui".to_string()];
    if let Some(command) = column_command_name(&args) {
        keys.push(format!("column.{}", command.to_lowercase()));
    }
    for key in keys {
        if let Some(value) = get_config_value(&key).ok().flatten() {
            if let Err(message) = apply_column_mode(&mut opts, &value) {
                eprintln!("{}", message);
                eprintln!("fatal: bad config variable '{}'", key);
                crate::ui::exit(128);
            }
        }
    }
    if let Err((message, code)) = parse_column_args(&args, &mut opts) {
        if message.starts_with("usage:") && args.iter().any(|a| a == "-h") {
            print!("{}", message);
        } else {
            eprint!("{}", message);
        }
        crate::ui::exit(code);
    }

    let mut input = Vec::new();
    io::stdin().lock().read_to_end(&mut input)?;
    let text = String::from_utf8_lossy(&input);
    let items: Vec<String> = text.lines().map(str::to_string).collect();
    io::stdout().lock().write_all(render_columns(&items, &opts).as_bytes())?;
    Ok(())
}
