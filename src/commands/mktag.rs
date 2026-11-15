use std::io::{self, Read};
use anyhow::Result;

use crate::helpers::mktag::check_tag_text;
use crate::helpers::objects::{read_object, write_object};
use crate::repo;

/// `rgit mktag [--no-strict]`
///
/// Reads the text of a tag object from standard input, checks it, writes it to the object
/// store and prints its hash. The text must be exactly `object <hash>`, `type <type>`,
/// `tag <name>` and `tagger <name> <email> <time> <zone>` lines, a blank line and the
/// message; the object must exist and have the stated type. Problems are reported with
/// git's fsck message ids. With `--no-strict`, a missing tagger, a tag name that is not a
/// valid ref name and extra header lines only give a warning. Unlike `tag`, no ref is
/// created. Exit status: 0, 128 when the tag is rejected, 129 for a usage error.
pub fn mktag(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit mktag\n\n    --[no-]strict         enable more strict checking\n\n";
    let mut strict = true;
    for arg in &args {
        match arg.as_str() {
            "--strict" => strict = true,
            "--no-strict" => strict = false,
            "-h" => {
                print!("{}", USAGE);
                std::process::exit(129);
            }
            other if other.starts_with("--") => {
                eprint!("error: unknown option `{}'\n{}", &other[2..], USAGE);
                std::process::exit(129);
            }
            other if other.starts_with('-') && other.len() > 1 => {
                eprint!("error: unknown switch `{}'\n{}", other.chars().nth(1).unwrap_or('-'), USAGE);
                std::process::exit(129);
            }
            // Like git, plain arguments are ignored.
            _ => {}
        }
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        std::process::exit(128);
    }

    let mut text = Vec::new();
    io::stdin().lock().read_to_end(&mut text)?;
    let (object, tagged_type, problems) = check_tag_text(&text, strict);
    let mut rejected = false;
    for (id, message, warning_only) in &problems {
        if *warning_only && !strict {
            eprintln!("warning: tag input does not pass fsck: {}: {}", id, message);
        } else {
            eprintln!("error: tag input does not pass fsck: {}: {}", id, message);
            rejected = true;
        }
    }
    if rejected {
        eprintln!("fatal: tag on stdin did not pass our strict fsck check");
        std::process::exit(128);
    }

    let (Some(object), Some(tagged_type)) = (object, tagged_type) else {
        eprintln!("fatal: tag on stdin did not pass our strict fsck check");
        std::process::exit(128);
    };
    match read_object(&object) {
        Ok((actual, _)) if actual != tagged_type => {
            eprintln!("fatal: object '{}' tagged as '{}', but is a '{}' type", object, tagged_type, actual);
            std::process::exit(128);
        }
        Ok(_) => {}
        Err(_) => {
            eprintln!("fatal: could not read tagged object '{}'", object);
            std::process::exit(128);
        }
    }
    println!("{}", write_object("tag", &text)?);
    Ok(())
}
