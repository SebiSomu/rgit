pub mod cli;
pub mod engine;
pub mod types;

use anyhow::Result;
use std::path::Path;
use crate::history::blame::engine::{blame_file, format_annotate_output, format_blame_output};
use crate::workspace::gitignore::normalize_path;
use crate::core::commit::resolve_commit_from_source;

/// `rgit blame [-L <start>,<end>] [<rev>] [--] <file>`
///
/// Shows, for every line of `<file>`, the commit that last changed it, its author and date.
/// Without `<rev>` the file as it is on disk is blamed against HEAD, so lines you have
/// edited but not committed show as "Not Committed Yet"; with `<rev>` that commit's version
/// of the file is blamed and the working copy is ignored. `-L` restricts the output to a
/// line range (`10,20`, `10,+5`, `10,`). Renames are not followed: a file that was renamed
/// is blamed on the commit that introduced it under its current name.
pub fn blame(args: Vec<String>, line_range: Option<String>) -> Result<()> {
    let (rev, file) = match args.as_slice() {
        [file] => (None, file.as_str()),
        [rev, file] => (Some(rev.as_str()), file.as_str()),
        _ => anyhow::bail!("usage: rgit blame [-L <start>,<end>] [<rev>] [--] <file>"),
    };

    let path = normalize_path(Path::new(file));
    let commit_hash = resolve_commit_from_source(rev.unwrap_or("HEAD"))?;
    let lines = blame_file(&path, &commit_hash, rev.is_none(), line_range.as_deref())?;
    print!("{}", format_blame_output(&lines));

    Ok(())
}

/// `rgit annotate [-L <start>,<end>] [<rev>] [--] <file>` (also `annotate <file> <rev>`)
///
/// `blame` in git's older `annotate` output format: every line shows the abbreviated commit,
/// then `(author<TAB>date<TAB>line number)` and the line, separated by tabs instead of
/// aligned columns. It takes the same arguments as `blame` and exists for scripts written
/// against `git annotate`; like git's, it also accepts the file before the revision.
pub fn annotate(args: Vec<String>, line_range: Option<String>) -> Result<()> {
    let (rev, file) = match args.as_slice() {
        [file] => (None, file.as_str()),
        [file, rev] if resolve_commit_from_source(rev).is_ok() && resolve_commit_from_source(file).is_err() => (Some(rev.as_str()), file.as_str()),
        [rev, file] => (Some(rev.as_str()), file.as_str()),
        _ => anyhow::bail!("usage: rgit annotate [-L <start>,<end>] [<rev>] [--] <file>"),
    };

    let path = normalize_path(Path::new(file));
    let commit_hash = resolve_commit_from_source(rev.unwrap_or("HEAD"))?;
    let lines = blame_file(&path, &commit_hash, rev.is_none(), line_range.as_deref())?;
    print!("{}", format_annotate_output(&lines));

    Ok(())
}
