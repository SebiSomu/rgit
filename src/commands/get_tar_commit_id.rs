use std::io::{self, Read};
use anyhow::Result;

/// `rgit get-tar-commit-id`
///
/// Reads a tar archive made by `archive` from standard input and prints the id of the commit
/// it was made from, which is stored as a comment in the archive's pax global header. Only
/// the first 1024 bytes are read. Prints nothing and exits 1 when the archive carries no
/// commit id (made from a tree, or not by `archive`). Works outside a repository.
pub fn get_tar_commit_id(args: Vec<String>) -> Result<()> {
    if !args.is_empty() {
        if args[0] == "-h" {
            println!("usage: rgit get-tar-commit-id");
        } else {
            eprintln!("usage: rgit get-tar-commit-id");
        }
        std::process::exit(129);
    }
    let mut header = [0u8; 1024];
    let mut filled = 0;
    let mut stdin = io::stdin().lock();
    while filled < header.len() {
        match stdin.read(&mut header[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    if filled < header.len() {
        eprintln!("fatal: git get-tar-commit-id: EOF before reading tar header");
        std::process::exit(128);
    }
    // Byte 156 is the entry type: 'g' is a pax global header, whose data follows in the next block.
    if header[156] != b'g' {
        std::process::exit(1);
    }
    let content = &header[512..];
    let Some(rest) = content.strip_prefix(b"52 comment=") else { std::process::exit(1) };
    if rest.len() < 41 || rest[40] != b'\n' || !rest[..40].iter().all(|b| b.is_ascii_hexdigit()) {
        std::process::exit(1);
    }
    println!("{}", String::from_utf8_lossy(&rest[..40]));
    Ok(())
}
