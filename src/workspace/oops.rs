pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;

use crate::workspace::oops::engine::collect_fixes;
use crate::workspace::oops::types::Fix;

/// How many suggestions `rgit oops` shows without `--all`.
const SHOWN: usize = 3;

/// `rgit oops`: looks at the operation log, HEAD's reflog and the repository's state, and lists
/// the most likely fixes for a mistake just made, each with the exact commands. With a number,
/// runs that fix.
pub fn oops(pick: Option<usize>, all: bool) -> Result<()> {
    let fixes = collect_fixes()?;
    match pick {
        None => list(&fixes, all),
        Some(n) => run(&fixes, n),
    }
}

/// Prints the suggestions, most likely first.
fn list(fixes: &[Fix], all: bool) -> Result<()> {
    if fixes.is_empty() {
        println!("Nothing looks wrong: no operation is half-done, no branch was deleted, no commits were left behind,");
        println!("and there's no recent command to take back.");
        println!("(`rgit undo --list` shows everything that can still be undone; `rgit reflog` shows where HEAD has been.)");
        return Ok(());
    }
    let shown = if all { fixes.len() } else { fixes.len().min(SHOWN) };
    println!("Most likely fixes, best guess first:");
    for (i, fix) in fixes.iter().take(shown).enumerate() {
        println!();
        println!("  {}) {}", i + 1, fix.title);
        for line in wrap(&fix.why, 92) {
            println!("     {}", line);
        }
        for command in &fix.commands {
            println!("     $ rgit {}", display(command));
        }
    }
    println!();
    println!("Run one with `rgit oops <number>` (it can be taken back with `rgit undo`).");
    if shown < fixes.len() {
        println!("{} more with `rgit oops --all`.", fixes.len() - shown);
    }
    Ok(())
}

/// Runs suggestion `n` (1-based), one rgit command at a time, stopping at the first failure.
/// Each command is a separate rgit process, so each is logged and undoable on its own.
fn run(fixes: &[Fix], n: usize) -> Result<()> {
    let Some(fix) = n.checked_sub(1).and_then(|i| fixes.get(i)) else {
        if fixes.is_empty() {
            eprintln!("error: there is no fix {}: nothing looks wrong right now.", n);
        } else {
            eprintln!("error: there is no fix {}: `rgit oops --all` lists {} (1 to {}).", n, plural(fixes.len(), "suggestion", "suggestions"), fixes.len());
        }
        crate::ui::exit(1);
    };
    println!("Fix {}: {}", n, fix.title);
    let exe = std::env::current_exe()?;
    for command in &fix.commands {
        println!("$ rgit {}", display(command));
        let status = std::process::Command::new(&exe).args(command).status()?;
        if !status.success() {
            eprintln!("error: `rgit {}` failed, so the rest of the fix was not run.", display(command));
            if fix.commands.len() > 1 {
                eprintln!("       `rgit undo --list` shows which steps ran; `rgit undo` takes them back one by one.");
            }
            crate::ui::exit(status.code().unwrap_or(1));
        }
    }
    // `undo` / `redo` already said how to reverse themselves.
    let reverses_itself = matches!(fix.commands.as_slice(), [c] if matches!(c.first().map(String::as_str), Some("undo" | "redo")));
    if !reverses_itself {
        let steps = fix.commands.len();
        println!(
            "Done. {}",
            if steps == 1 { "`rgit undo` takes it back.".to_string() } else { format!("`rgit undo` takes the steps back one at a time ({} steps).", steps) }
        );
    }
    Ok(())
}

/// A command line for display, quoting arguments with spaces.
fn display(command: &[String]) -> String {
    command.iter().map(|a| if a.is_empty() || a.contains(char::is_whitespace) { format!("{:?}", a) } else { a.clone() }).collect::<Vec<_>>().join(" ")
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// Splits text into lines of at most `width` characters, at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
