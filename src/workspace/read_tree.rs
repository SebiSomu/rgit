pub mod cli;
pub mod engine;
pub mod types;

use std::fs;
use std::path::Path;
use anyhow::Result;

use crate::workspace::index_plumbing::write_entry_to_worktree;
use crate::workspace::read_tree::engine::{parse_read_tree_args, plan_read_tree};
use crate::core::index::{build_entry, read_index, write_index};
use crate::core::repo;

/// `rgit read-tree [-m [--trivial] [--aggressive] | --reset | --prefix=<dir>/] [-u | -i]
/// [-n] (--empty | <tree-ish>...)`
///
/// Reads tree information into the index.
/// - `<tree-ish>` alone replaces the index with the tree (several trees are overlaid, later
///   ones winning); `--empty` empties it; `--prefix=<dir>/` adds the tree below a directory
///   and keeps everything else, failing if entries overlap.
/// - `-m <tree>` does the same as a merge: entries that do not change keep their stat data,
///   and an entry that would change must not have local modifications in the working tree.
/// - `-m <old> <new>` is the two-way merge used when switching branches: changes staged on
///   top of `<old>` are carried over to `<new>` unless they clash.
/// - `-m <base>... <ours> <theirs>` is the three-way merge, resolving only the trivial cases
///   (one side changed, or both made the same change); `--aggressive` also resolves
///   deletions and identical additions.
/// `-u` updates the working tree to match (never overwriting untracked files), `-i` ignores
/// the working tree, `--reset` proceeds even over local changes, `-n` only checks.
/// Where git would leave a path unmerged with stages 1-3 for a later file-level merge, rgit
/// fails instead, because its index has no conflict stages — use `merge` or `merge-tree`
/// for those. Exit status: 0, 128 on errors (nothing is changed), 129 for a usage error.
pub fn read_tree(args: Vec<String>) -> Result<()> {
    let opts = match parse_read_tree_args(&args) {
        Ok(opts) => opts,
        Err((message, code)) => {
            if message.starts_with("usage:") {
                print!("{}", message);
            } else {
                eprint!("{}", message);
            }
            crate::ui::exit(code);
        }
    };
    if !repo::exists() {
        eprintln!("fatal: not a git repository (or any of the parent directories): .git");
        crate::ui::exit(128);
    }
    if opts.trees.is_empty() {
        if !opts.empty {
            eprintln!("warning: read-tree: emptying the index with no arguments is deprecated; use --empty");
        }
        write_index(&mut Vec::new())?;
        return Ok(());
    }

    let index = read_index()?;
    let mut plan = match plan_read_tree(&opts, &index) {
        Ok(plan) => plan,
        Err(message) => {
            eprintln!("{}", message);
            crate::ui::exit(128);
        }
    };
    if !plan.errors.is_empty() {
        // Like git's plumbing, stop at the first problem.
        eprintln!("{}", plan.errors[0]);
        crate::ui::exit(128);
    }
    if !plan.unmerged.is_empty() {
        if opts.trivial {
            eprintln!("fatal: Merge requires file-level merging");
        } else {
            for path in &plan.unmerged {
                eprintln!("error: Entry '{}' needs a file-level merge.", path);
            }
            eprintln!("fatal: rgit's index has no conflict stages, so read-tree cannot leave paths unmerged; use merge or merge-tree");
        }
        crate::ui::exit(128);
    }
    if opts.dry_run {
        return Ok(());
    }

    if opts.update {
        for path in &plan.remove {
            let file = Path::new(path);
            if file.is_file() {
                fs::remove_file(file)?;
                // Remove directories the file leaves empty.
                let mut parent = file.parent();
                while let Some(dir) = parent.filter(|d| !d.as_os_str().is_empty()) {
                    if fs::remove_dir(dir).is_err() {
                        break;
                    }
                    parent = dir.parent();
                }
            }
        }
        for path in &plan.write {
            let Some(entry) = plan.entries.iter_mut().find(|e| &e.path == path) else { continue };
            if let Err(message) = write_entry_to_worktree(entry, Path::new(path), true) {
                eprintln!("{}", message);
                crate::ui::exit(128);
            }
            if let Ok(metadata) = fs::metadata(path) {
                let mode = entry.mode;
                *entry = build_entry(path, entry.hash, &metadata);
                entry.mode = mode;
            }
        }
    }
    write_index(&mut plan.entries)?;
    Ok(())
}
