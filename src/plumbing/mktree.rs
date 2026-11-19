pub mod cli;
pub mod engine;

use std::io::{self, Read};
use anyhow::Result;

use crate::plumbing::mktree::engine::{parse_mktree_line, write_tree_from_entries, MKTREE_USAGE};
use crate::core::repo;

/// `rgit mktree [-z] [--missing] [--batch]`
///
/// Builds a tree object from `ls-tree` formatted text on standard input
/// (`<mode> SP <type> SP <hash> TAB <name>` per line, in any order) and prints its hash.
/// Entries are single names: a subdirectory is given as a `tree` entry, not as a path with
/// slashes. Every object must exist and have the type its mode implies, unless `--missing`
/// is given (submodule `commit` entries may always be missing). `-z` reads NUL-terminated
/// lines with unquoted names; `--batch` builds one tree per block of lines, blocks being
/// separated by an empty line. Exit status: 0, 128 for bad input, 129 for a usage error.
pub fn mktree(args: Vec<String>) -> Result<()> {
    let (mut nul, mut missing, mut batch) = (false, false, false);
    for arg in &args {
        match arg.as_str() {
            "-z" => nul = true,
            "--missing" => missing = true,
            "--no-missing" => missing = false,
            "--batch" => batch = true,
            "--no-batch" => batch = false,
            "-h" => {
                print!("{}", MKTREE_USAGE);
                crate::ui::exit(129);
            }
            other if other.starts_with("--") => {
                eprint!("error: unknown option `{}'\n{}", &other[2..], MKTREE_USAGE);
                crate::ui::exit(129);
            }
            other if other.starts_with('-') && other.len() > 1 => {
                eprint!("error: unknown switch `{}'\n{}", other.chars().nth(1).unwrap_or('-'), MKTREE_USAGE);
                crate::ui::exit(129);
            }
            // Like git, plain arguments are ignored.
            _ => {}
        }
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }

    let mut input = Vec::new();
    io::stdin().lock().read_to_end(&mut input)?;
    let terminator = if nul { 0u8 } else { b'\n' };
    let mut lines: Vec<&[u8]> = input.split(|b| *b == terminator).collect();
    // The text after the last terminator is not a line unless something is there.
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }

    let fail = |message: String| -> ! {
        eprintln!("{}", message);
        crate::ui::exit(128);
    };
    let mut entries: Vec<(u32, [u8; 20], Vec<u8>)> = Vec::new();
    let mut in_block = false;
    for line in lines {
        if line.is_empty() {
            if !batch {
                fail("fatal: input format error: (blank line only valid in batch mode)".to_string());
            }
            println!("{}", write_tree_from_entries(&mut entries)?);
            entries.clear();
            in_block = false;
            continue;
        }
        match parse_mktree_line(line, nul, missing) {
            Ok(entry) => entries.push(entry),
            Err(message) => fail(message),
        }
        in_block = true;
    }
    // In batch mode, input that ends without a pending entry does not make one more (empty) tree.
    if in_block || !batch {
        println!("{}", write_tree_from_entries(&mut entries)?);
    }
    Ok(())
}
