use std::io::{self, Read, Write};
use anyhow::Result;

use crate::helpers::config::get_config_value;
use crate::helpers::stripspace::{comment_lines, strip_space, STRIPSPACE_USAGE};

/// `rgit stripspace [-s | --strip-comments]` / `rgit stripspace [-c | --comment-lines]`
///
/// Filters standard input the way git cleans up commit messages, tag messages and notes:
/// trailing whitespace is removed from every line, several empty lines in a row become one,
/// empty lines at the beginning and end are dropped, and the text ends with a newline. `-s`
/// also removes the lines starting with the comment character (`#`, or `core.commentChar`);
/// `-c` instead only prepends the comment character and a space to every line. Works outside
/// a repository. Exit status: 0, or 129 for a usage error.
pub fn stripspace(args: Vec<String>) -> Result<()> {
    let mut mode: Option<&str> = None;
    for arg in &args {
        let this = match arg.as_str() {
            "-s" | "--strip-comments" => "-s",
            "-c" | "--comment-lines" => "-c",
            "-h" => {
                print!("{}", STRIPSPACE_USAGE);
                std::process::exit(129);
            }
            other if other.starts_with("--") && other.len() > 2 => {
                eprint!("error: unknown option `{}'\n{}", &other[2..], STRIPSPACE_USAGE);
                std::process::exit(129);
            }
            other if other.starts_with('-') && other.len() > 1 => {
                eprint!("error: unknown switch `{}'\n{}", other.chars().nth(1).unwrap_or('-'), STRIPSPACE_USAGE);
                std::process::exit(129);
            }
            _ => {
                eprint!("{}", STRIPSPACE_USAGE);
                std::process::exit(129);
            }
        };
        if let Some(previous) = mode.filter(|m| *m != this) {
            eprintln!("error: {} is incompatible with {}", this, previous);
            std::process::exit(129);
        }
        mode = Some(this);
    }

    let comment_char = get_config_value("core.commentchar")
        .ok()
        .flatten()
        .and_then(|v| v.bytes().next())
        .unwrap_or(b'#');

    let mut input = Vec::new();
    io::stdin().lock().read_to_end(&mut input)?;
    let output = match mode {
        Some("-c") => comment_lines(&input, comment_char),
        Some(_) => strip_space(&input, true, comment_char),
        None => strip_space(&input, false, comment_char),
    };
    io::stdout().lock().write_all(&output)?;
    Ok(())
}
