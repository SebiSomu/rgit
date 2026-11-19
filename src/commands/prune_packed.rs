use anyhow::Result;

use crate::helpers::pack::prune_packed_objects;
use crate::repo;

/// `rgit prune-packed [-n | --dry-run] [-q | --quiet]`
///
/// Removes loose objects that are also stored in a packfile, since the packed copy is
/// enough; directories left empty are removed too. `-n` only prints what would be removed,
/// as `rm -f <path>` lines. `-q` is accepted (there is no progress output to silence).
/// Exit status: 0, 129 for a usage error.
pub fn prune_packed(args: Vec<String>) -> Result<()> {
    const USAGE: &str = "usage: rgit prune-packed [-n | --dry-run] [-q | --quiet]\n\n    -n, --[no-]dry-run    dry run\n    -q, --[no-]quiet      be quiet\n\n";
    let mut dry_run = false;
    for arg in &args {
        match arg.as_str() {
            "-n" | "--dry-run" => dry_run = true,
            "--no-dry-run" => dry_run = false,
            "-q" | "--quiet" | "--no-quiet" => {}
            "-h" => {
                print!("{}", USAGE);
                crate::ui::exit(129);
            }
            other => {
                let shown = if let Some(long) = other.strip_prefix("--") { format!("error: unknown option `{}'\n", long) } else if other.starts_with('-') { format!("error: unknown switch `{}'\n", &other[1..2.min(other.len())]) } else { String::new() };
                eprint!("{}{}", shown, USAGE);
                crate::ui::exit(129);
            }
        }
    }
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    for path in prune_packed_objects(dry_run)? {
        if dry_run {
            println!("rm -f {}", path);
        }
    }
    Ok(())
}
