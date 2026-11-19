use std::collections::BTreeMap;
use crate::helpers::format_patch::parse_commit_object;
use crate::helpers::ls_files::{matches_ls_files_pathspec, normalize_pathspecs};
use crate::core::object_store::flatten_tree;
use crate::helpers::rev_list::{parse_revision_args, select_rev_list_commits};
use crate::objects::LastModifiedOptions;

/// Usage text for `last-modified`, printed for `-h` and after option errors (exit 129).
pub const LAST_MODIFIED_USAGE: &str = "usage: rgit last-modified [--recursive] [--show-trees] [-z] [<revision-range>] [[--] <path>...]

    -r, --recursive       recurse into subtrees
    -t, --show-trees      show tree entries when recursing into subtrees
    -z                    lines are separated by NUL character
";

/// Parses `last-modified` arguments: `-r`, `-t` (which implies `-r`), `-z`, an optional
/// revision or range, and paths (everything after `--`, or arguments that aren't revisions).
/// Errors carry the usage text and mean exit 129.
// Used by `last-modified`.
pub fn parse_last_modified_args(args: &[String]) -> anyhow::Result<LastModifiedOptions> {
    let mut opts = LastModifiedOptions::default();
    let mut rev_args: Vec<String> = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "--" => {
                rev_args.extend(args[i..].iter().cloned());
                break;
            }
            "-r" | "--recursive" => opts.recursive = true,
            "-t" | "--show-trees" => opts.show_trees = true,
            "-z" => opts.nul = true,
            other if other.starts_with('-') && other.len() > 1 && !other.starts_with("--not") => {
                anyhow::bail!("error: unknown option `{}'\n{}", other.trim_start_matches('-'), LAST_MODIFIED_USAGE);
            }
            _ => rev_args.push(arg.clone()),
        }
    }
    opts.recursive = opts.recursive || opts.show_trees;
    opts.revs = parse_revision_args(&rev_args)?;
    if opts.revs.revs.is_empty() {
        opts.revs.revs.push("HEAD".to_string());
    }
    Ok(opts)
}

/// Finds, for every path of the newest commit in the range, the commit that last changed
/// it: the first commit in `log -- <path>` order (so a merge that took a path unchanged
/// from a parent is skipped in favour of that parent's history). Without `recursive` only
/// the top-level entries are listed (a directory counts as changed when anything inside it
/// did); with it every file, plus the directories with `show_trees`. Path arguments limit
/// the listing. A path nothing in the range touched is reported against the range's
/// boundary with `boundary` set. Returns `(commit, path, boundary)` in path order.
// Used by `last-modified`.
pub fn last_modified_paths(opts: &LastModifiedOptions) -> anyhow::Result<Vec<(String, String, bool)>> {
    let mut tips = opts.revs.clone();
    tips.paths.clear();
    tips.max_count = Some(1);
    let (newest, _) = select_rev_list_commits(&tips)?;
    let Some(tip) = newest.first() else { return Ok(Vec::new()) };

    let mut files = BTreeMap::new();
    flatten_tree(&tip.commit.tree, "", &mut files)?;
    let specs = normalize_pathspecs(&opts.revs.paths);
    let mut targets: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for path in files.keys().filter(|path| specs.is_empty() || matches_ls_files_pathspec(path, &specs)) {
        // Below a path argument that names a directory, list that directory's own entries.
        let base = specs.iter().filter(|spec| path.starts_with(&format!("{}/", spec.trim_end_matches('/')))).map(|spec| spec.trim_end_matches('/').len() + 1).max().unwrap_or(0);
        let parts: Vec<&str> = path[base..].split('/').collect();
        if opts.recursive {
            targets.insert(path.clone());
            if opts.show_trees {
                for depth in 1..parts.len() {
                    targets.insert(format!("{}{}", &path[..base], parts[..depth].join("/")));
                }
            }
        } else {
            targets.insert(format!("{}{}", &path[..base], parts[0]));
        }
    }

    // The boundary of a range such as A..B is A: the commit paths untouched by the range are reported against.
    let boundary = opts.revs.revs.iter().find_map(|rev| rev.strip_prefix('^').map(str::to_string).or_else(|| rev.split_once("..").map(|(a, _)| a.to_string()).filter(|a| !a.is_empty())));
    let boundary = boundary.and_then(|rev| crate::core::rev_parse::resolve_rev(&rev).ok()).and_then(|hash| crate::core::rev_parse::peel_to_type(hash, "commit").ok());

    let mut out = Vec::new();
    for path in targets {
        let mut walk = opts.revs.clone();
        walk.paths = vec![path.clone()];
        walk.max_count = Some(1);
        let (found, _) = select_rev_list_commits(&walk)?;
        match (found.first(), &boundary) {
            (Some(walked), _) => out.push((walked.commit.hash.clone(), path, false)),
            (None, Some(boundary)) => out.push((boundary.clone(), path, true)),
            (None, None) => {
                // No range: the path came in with the root commit of this history.
                let mut root = parse_commit_object(&tip.commit.hash)?;
                while let Some(parent) = root.parents.first().cloned() {
                    root = parse_commit_object(&parent)?;
                }
                out.push((root.hash, path, false));
            }
        }
    }
    Ok(out)
}
