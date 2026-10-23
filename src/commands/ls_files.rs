use std::collections::{BTreeSet, HashMap};
use std::io::{self, BufWriter, Write};
use anyhow::Result;

use crate::helpers::ls_files::{collect_untracked_paths, is_tracked_path_ignored, matches_ls_files_pathspec, normalize_pathspecs, worktree_state};
use crate::index::{read_index, IndexEntry};
use crate::objects::{LsFilesOptions, WorktreeState};

/// `rgit ls-files [-c] [-d] [-m] [-o] [-i] [--exclude-standard] [-s] [-t] [-z] [<pathspec>...]`
///
/// Shows information about files in the index and the working tree.
/// With no mode flag, lists every tracked path (`--cached`). `-m`/`-d` list tracked paths whose
/// working-tree file was modified/deleted (`-m` also includes deleted files, like git), `-o` lists
/// untracked files, and `-i` restricts output to `.gitignore`-excluded paths. As in git, untracked
/// files are printed first, then index entries in path order; a single index entry may be printed
/// once per matching mode (e.g. both cached and modified).
pub fn ls_files(opts: LsFilesOptions) -> Result<()> {
    if opts.ignored && !opts.others && !opts.cached {
        anyhow::bail!("fatal: ls-files -i must be used with either -o or -c");
    }
    if opts.ignored && !opts.exclude_standard {
        anyhow::bail!("fatal: ls-files --ignored needs some exclude pattern");
    }

    let show_index = opts.cached || opts.stage || !(opts.deleted || opts.modified || opts.others);
    let specs = normalize_pathspecs(&opts.pathspecs);
    let entries = read_index()?;
    let terminator = if opts.null_terminated { '\0' } else { '\n' };
    let tag = |t: &'static str| if opts.show_tags { t } else { "" };

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    if opts.others {
        let tracked: BTreeSet<String> = entries.iter().map(|e| e.path.clone()).collect();
        let descend_ignored = opts.ignored || !opts.exclude_standard;
        for untracked in collect_untracked_paths(&tracked, descend_ignored)? {
            let wanted = if opts.ignored {
                untracked.ignored
            } else {
                !opts.exclude_standard || !untracked.ignored
            };
            if wanted && matches_ls_files_pathspec(&untracked.path, &specs) {
                write!(out, "{}{}{}", tag("? "), untracked.path, terminator)?;
            }
        }
    }

    let check_worktree = opts.modified || opts.deleted;
    let mut ignore_cache = HashMap::new();

    for entry in &entries {
        if !matches_ls_files_pathspec(&entry.path, &specs) {
            continue;
        }
        if opts.ignored && !is_tracked_path_ignored(&entry.path, &mut ignore_cache) {
            continue;
        }

        if show_index {
            write_entry(&mut out, entry, tag("H "), opts.stage, terminator)?;
        }

        if check_worktree {
            let state = worktree_state(entry)?;
            if opts.deleted && state == WorktreeState::Deleted {
                write_entry(&mut out, entry, tag("R "), opts.stage, terminator)?;
            }
            if opts.modified && state != WorktreeState::Unchanged {
                write_entry(&mut out, entry, tag("C "), opts.stage, terminator)?;
            }
        }
    }

    out.flush()?;
    Ok(())
}

fn write_entry(out: &mut impl Write, entry: &IndexEntry, tag: &str, stage: bool, terminator: char) -> io::Result<()> {
    if stage {
        write!(out, "{}{:06o} {} 0\t{}{}", tag, entry.mode, hex::encode(entry.hash), entry.path, terminator)
    } else {
        write!(out, "{}{}{}", tag, entry.path, terminator)
    }
}
