use std::io::{self, BufRead, Write};
use anyhow::Result;

use crate::mail::mailmap::{map_user, parse_check_mailmap_args, read_mailmap, split_contact, CHECK_MAILMAP_USAGE};
use crate::objects::Mailmap;
use crate::core::repo;

/// `rgit check-mailmap [--stdin] [--mailmap-file <file>] [--mailmap-blob <blob>] <contact>...`
///
/// Prints the canonical name and email of each contact (`Name <email>`, or a bare
/// `<email>`) according to the mailmap: `.mailmap` at the top of the working tree, the blob
/// named by `mailmap.blob`, the file named by `mailmap.file`, and the `--mailmap-blob` /
/// `--mailmap-file` given here, later sources winning. A `.mailmap` line maps a commit
/// email (optionally together with a commit name) to the proper name and/or email:
/// `Proper Name <commit@email>`, `<proper@email> <commit@email>`, `Proper Name
/// <proper@email> <commit@email>`, or `Proper Name <proper@email> Commit Name
/// <commit@email>`. Emails and names are matched ignoring case; contacts without a mapping
/// are printed unchanged. `--stdin` also reads contacts from standard input, one per line.
/// An address without `<>` is taken as a bare email (as in git 2.47 and later). Exit status,
/// like git: 0 success, 128 no contacts or outside a repository, 129 usage.
pub fn check_mailmap(args: Vec<String>) -> Result<()> {
    if args.iter().take_while(|a| a.as_str() != "--").any(|a| a == "-h") {
        print!("{}", CHECK_MAILMAP_USAGE);
        crate::ui::exit(129);
    }
    let opts = match parse_check_mailmap_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            eprint!("{}", e);
            crate::ui::exit(129);
        }
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    if opts.contacts.is_empty() && !opts.stdin {
        eprintln!("fatal: no contacts specified");
        crate::ui::exit(128);
    }

    let map = read_mailmap(opts.mailmap_file.as_deref(), opts.mailmap_blob.as_deref());
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for contact in &opts.contacts {
        print_mapped_contact(&mut out, &map, contact)?;
    }
    out.flush()?;

    if opts.stdin {
        for line in io::stdin().lock().split(b'\n') {
            print_mapped_contact(&mut out, &map, &String::from_utf8_lossy(&line?))?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Prints one contact after mapping it: `Name <email>`, or just `<email>` when there is no name.
fn print_mapped_contact(out: &mut impl Write, map: &Mailmap, contact: &str) -> Result<()> {
    let (name, email) = split_contact(contact).unwrap_or_else(|| (String::new(), contact.to_string()));
    let (name, email) = map_user(map, &name, &email);
    if !name.is_empty() {
        write!(out, "{} ", name)?;
    }
    writeln!(out, "<{}>", email)?;
    Ok(())
}
