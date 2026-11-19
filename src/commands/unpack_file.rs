use anyhow::Result;

use crate::helpers::index_plumbing::write_merge_temp_file;
use crate::core::object_store::read_object;
use crate::core::rev_parse::resolve_rev;
use crate::core::repo;

/// `rgit unpack-file <blob>`
///
/// Writes the contents of a blob to a new temporary file named `.merge_file_XXXXXX` in the
/// current directory and prints the file's name. Meant for scripts that need a blob as a
/// real file (for example to hand it to `merge-file`). Exit status: 0, 128 when the blob
/// cannot be read, 129 for a usage error.
pub fn unpack_file(args: Vec<String>) -> Result<()> {
    if args.len() != 1 || args[0] == "-h" {
        if args.first().is_some_and(|a| a == "-h") {
            println!("usage: rgit unpack-file <blob>");
        } else {
            eprintln!("usage: rgit unpack-file <blob>");
        }
        crate::ui::exit(129);
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    let Ok(hash) = resolve_rev(&args[0]) else {
        eprintln!("fatal: Not a valid object name {}", args[0]);
        crate::ui::exit(128);
    };
    let content = match read_object(&hash) {
        Ok((object_type, content)) if object_type == "blob" => content,
        _ => {
            eprintln!("fatal: unable to read blob object {}", hash);
            crate::ui::exit(128);
        }
    };

    match write_merge_temp_file(&content) {
        Ok(name) => println!("{}", name),
        Err(e) => {
            eprintln!("fatal: unable to create temporary file: {}", e);
            crate::ui::exit(128);
        }
    }
    Ok(())
}
