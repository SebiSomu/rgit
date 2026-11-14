use std::fs;
use std::io::{self, Read};
use std::path::Path;
use anyhow::Result;

use crate::helpers::mailsplit::{first_maildir_message, list_maildir_files, parse_mailsplit_args, split_mbox_messages, write_split_message, MAILSPLIT_USAGE};
use crate::objects::MailsplitOptions;

/// `rgit mailsplit [-d<prec>] [-f<n>] [-b] [--keep-cr] [--mboxrd] -o<directory> [(<mbox>|<Maildir>)...]`
///
/// Splits mbox files (or standard input, or `-`) into one file per message, written to
/// `<directory>` as `0001`, `0002`, ... and prints how many were written. Messages are cut
/// at mbox `From <sender> <date>` separator lines, which stay as the first line of each
/// file, exactly as `am` and `mailinfo` expect. `-d<prec>` changes the number of digits
/// (3 to 9), `-f<n>` starts numbering after `n`, `-b` accepts a single mail without a
/// separator line, `--keep-cr` keeps CRLF line endings, and `--mboxrd` undoes `>From `
/// quoting. A directory argument is read as a Maildir (`cur/` and `new/`). The old forms
/// `mailsplit <mbox> <directory>` and `mailsplit <directory>` work too. Works outside a
/// repository. Exit status, like git: 0 success, 1 a mailbox that couldn't be split, 128
/// fatal, 129 usage.
pub fn mailsplit(args: Vec<String>) -> Result<()> {
    if args.first().is_some_and(|a| a == "-h" || a == "--help") {
        print!("{}", MAILSPLIT_USAGE);
        std::process::exit(129);
    }
    let opts = match parse_mailsplit_args(&args) {
        Ok(opts) => opts,
        Err(e) => {
            let message = e.to_string();
            if message.starts_with("usage:") {
                eprint!("{}", message);
                std::process::exit(129);
            }
            eprintln!("{}", message);
            std::process::exit(128);
        }
    };

    let mut number = opts.skip;
    let mut written = 0;
    for input in &opts.inputs {
        let result = if input == "-" {
            split_mbox_input(input, &opts, number)
        } else {
            match fs::metadata(input) {
                Err(e) => {
                    eprintln!("error: cannot stat {}: {}", input, io_reason(&e));
                    std::process::exit(1);
                }
                Ok(meta) if meta.is_dir() => split_maildir_input(input, &opts, number),
                Ok(_) => split_mbox_input(input, &opts, number),
            }
        };
        match result {
            Some(last) => {
                written += last - number;
                number = last;
            }
            None => {
                eprintln!("error: cannot split patches from {}", if input == "-" { "stdin" } else { input });
                std::process::exit(1);
            }
        }
    }
    println!("{}", written);
    Ok(())
}

/// Splits one mbox (or standard input for `-`) into numbered files and returns the last
/// number used, or `None` after reporting why it couldn't be split.
fn split_mbox_input(input: &str, opts: &MailsplitOptions, mut number: usize) -> Option<usize> {
    let from_stdin = input == "-";
    let mut data = Vec::new();
    let read = if from_stdin { io::stdin().lock().read_to_end(&mut data).map(|_| ()) } else { fs::read(input).map(|bytes| data = bytes) };
    if let Err(e) = read {
        eprintln!("error: cannot open mbox {}: {}", input, io_reason(&e));
        return None;
    }

    let messages = match split_mbox_messages(&data, opts) {
        Ok(Some(messages)) => messages,
        Ok(None) if from_stdin => return Some(number),
        Ok(None) => {
            eprintln!("error: empty mbox: '{}'", input);
            return None;
        }
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    for message in messages {
        number += 1;
        write_or_die(opts, number, &message);
    }
    Some(number)
}

/// Copies every mail of a Maildir into numbered files (each is one bare message) and
/// returns the last number used, or `None` after reporting a mail that couldn't be read.
fn split_maildir_input(input: &str, opts: &MailsplitOptions, mut number: usize) -> Option<usize> {
    let files = match list_maildir_files(Path::new(input)) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("{:#}", e);
            return None;
        }
    };
    for file in files {
        let path = format!("{}/{}", input, file);
        let data = match fs::read(&path) {
            Ok(data) if !data.is_empty() => data,
            Ok(_) => {
                eprintln!("error: cannot read mail {}", path);
                return None;
            }
            Err(e) => {
                eprintln!("error: cannot open mail {}: {}", path, io_reason(&e));
                return None;
            }
        };
        number += 1;
        write_or_die(opts, number, &first_maildir_message(&data, opts));
    }
    Some(number)
}

fn write_or_die(opts: &MailsplitOptions, number: usize, message: &[u8]) {
    if let Err(e) = write_split_message(&opts.directory, opts.precision, number, message) {
        eprintln!("{:#}", e);
        std::process::exit(128);
    }
}

fn io_reason(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => "No such file or directory".to_string(),
        io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        _ => e.to_string(),
    }
}
