use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::helpers::add_patch::{
    apply_patch_file, parse_patch_file, run_patch_mode, MODE_ADD, MODE_CHECKOUT_HEAD, MODE_CHECKOUT_INDEX, MODE_CHECKOUT_NOTHEAD,
    MODE_RESET_HEAD, MODE_RESET_NOTHEAD, MODE_STASH, MODE_WORKTREE_HEAD, MODE_WORKTREE_NOTHEAD,
};
use crate::helpers::index_plumbing::{bare_index_entry, worktree_file_mode};
use crate::helpers::ls_files::{matches_ls_files_pathspec, normalize_pathspecs};
use crate::helpers::{
    append_stash_entry, build_commit, build_merge_commit, build_tree_from_map, commit_subject_line, current_branch_label, flatten_tree, hash_content,
    read_object, resolve_tree_from_source, tree_hash_of_commit, write_object, write_tree_from_index_prefix,
};
use crate::index::{read_index, write_index};
use crate::objects::{InteractiveFile, PatchModeSpec, PatchTarget};
use crate::refs;

type TreeMap = BTreeMap<String, ([u8; 20], u32)>;

/// One side of a patch-mode diff.
enum Side {
    Tree(TreeMap),
    Index,
    Worktree,
}

/// `rgit add -p [<pathspec>...]`: stage hunks of tracked files, one at a time.
pub fn add_patch(paths: Vec<String>) -> anyhow::Result<()> {
    run_mode(&MODE_ADD, Side::Index, Side::Worktree, &paths)
}

/// `rgit reset -p [<tree-ish>] [--] [<pathspec>...]`: unstage hunks (or, with a tree-ish other
/// than `HEAD`, copy its hunks into the index).
pub fn reset_patch(args: Vec<String>) -> anyhow::Result<()> {
    let (rev, paths) = split_rev_and_paths(&args, &["-p", "--patch", "-q", "--quiet", "--mixed"])?;
    match rev.as_deref() {
        None | Some("HEAD") => run_mode(&MODE_RESET_HEAD, Side::Tree(head_tree()?), Side::Index, &paths),
        Some(rev) => run_mode(&MODE_RESET_NOTHEAD, Side::Index, Side::Tree(resolve_tree_from_source(rev)?), &paths),
    }
}

/// `rgit checkout -p [<tree-ish>] [--] [<pathspec>...]`: discard working-tree hunks (back to the
/// index), discard hunks from index and working tree (`HEAD`), or copy another tree's hunks
/// into both.
pub fn checkout_patch(args: Vec<String>) -> anyhow::Result<()> {
    let (rev, paths) = split_rev_and_paths(&args, &["-p", "--patch", "-q", "--quiet"])?;
    match rev.as_deref() {
        None => run_mode(&MODE_CHECKOUT_INDEX, Side::Index, Side::Worktree, &paths),
        Some("HEAD") => run_mode(&MODE_CHECKOUT_HEAD, Side::Tree(head_tree()?), Side::Worktree, &paths),
        Some(rev) => run_mode(&MODE_CHECKOUT_NOTHEAD, Side::Worktree, Side::Tree(resolve_tree_from_source(rev)?), &paths),
    }
}

/// `rgit restore -p [-S] [-W] [--source=<tree>] [<pathspec>...]`, mapped onto git's modes: the
/// working tree alone (from the index, `HEAD`, or another tree), the index alone (like
/// `reset -p`), or both (like `checkout -p <tree>`).
pub fn restore_patch(paths: Vec<String>, staged: bool, worktree: bool, source: Option<String>) -> anyhow::Result<()> {
    let worktree = worktree || !staged;
    let source = source.or_else(|| staged.then(|| "HEAD".to_string()));
    match (staged, worktree, source.as_deref()) {
        (false, _, None) => run_mode(&MODE_CHECKOUT_INDEX, Side::Index, Side::Worktree, &paths),
        (false, _, Some("HEAD")) => run_mode(&MODE_WORKTREE_HEAD, Side::Tree(head_tree()?), Side::Worktree, &paths),
        (false, _, Some(rev)) => run_mode(&MODE_WORKTREE_NOTHEAD, Side::Worktree, Side::Tree(resolve_tree_from_source(rev)?), &paths),
        (true, false, Some("HEAD") | None) => run_mode(&MODE_RESET_HEAD, Side::Tree(head_tree()?), Side::Index, &paths),
        (true, false, Some(rev)) => run_mode(&MODE_RESET_NOTHEAD, Side::Index, Side::Tree(resolve_tree_from_source(rev)?), &paths),
        (true, true, Some("HEAD") | None) => run_mode(&MODE_CHECKOUT_HEAD, Side::Tree(head_tree()?), Side::Worktree, &paths),
        (true, true, Some(rev)) => run_mode(&MODE_CHECKOUT_NOTHEAD, Side::Worktree, Side::Tree(resolve_tree_from_source(rev)?), &paths),
    }
}

/// `rgit stash push -p [-m <message>] [-- <pathspec>...]`: stash only the chosen hunks (between
/// `HEAD` and the working tree) and take them out of the working tree; the index is kept, as with
/// git (`--patch` implies `--keep-index`).
pub fn stash_patch(message: Option<String>, paths: Vec<String>) -> anyhow::Result<()> {
    let head_commit = refs::resolve_head_commit()?.ok_or_else(|| anyhow::anyhow!("fatal: You do not have the initial commit yet"))?;
    let head = head_tree()?;
    // git runs this against a scratch index made from HEAD, so only HEAD's paths take part.
    let mut files = collect_files(&Side::Tree(head.clone()), &Side::Worktree, &paths, true)?;
    let index_entries = read_index().unwrap_or_default();
    let index_tree = write_tree_from_index_prefix(&index_entries, "")?;
    if files.is_empty() && index_tree == tree_hash_of_commit(&head_commit)? {
        println!("No local changes to save");
        return Ok(());
    }

    let mut stashed = head.clone();
    let mut chosen: Vec<InteractiveFile> = Vec::new();
    run_patch_mode(&MODE_STASH, &mut files, &mut |file, _ask| {
        let current = stashed.get(&file.path).map(|&(h, m)| (h, m));
        let bytes = match current {
            Some((h, _)) => read_object(&hex::encode(h))?.1,
            None => Vec::new(),
        };
        match apply_patch_file(file, &MODE_STASH, current.map(|(_, m)| (m, bytes.as_slice())), "index") {
            Ok(Some((content, mode))) => {
                let hash = write_object("blob", &content)?;
                stashed.insert(file.path.clone(), (hex_to_bytes(&hash), mode));
            }
            Ok(None) => {
                stashed.remove(&file.path);
            }
            Err(message) => eprintln!("{}", message),
        }
        chosen.push(file.clone());
        Ok(())
    })?;
    if stashed == head {
        eprintln!("No changes selected");
        crate::ui::exit(1);
    }

    // The stash: index as it is, working tree = HEAD plus the chosen hunks.
    let branch = current_branch_label()?;
    let short = &head_commit[..7];
    let subject = commit_subject_line(&head_commit)?;
    let index_commit = build_commit(index_tree, Some(head_commit.clone()), &format!("index on {}: {} {}", branch, short, subject))?;
    let stash_message = match &message {
        Some(m) => format!("On {}: {}", branch, m),
        None => format!("WIP on {}: {} {}", branch, short, subject),
    };
    let worktree_commit = build_merge_commit(build_tree_from_map(&stashed)?, &head_commit, &index_commit, &stash_message)?;
    append_stash_entry(&worktree_commit, &stash_message)?;
    println!("Saved working directory and index state {}", stash_message);

    // Take the chosen hunks back out of the working tree, all files or none (one `apply -R`).
    let reverse = PatchModeSpec { is_reverse: true, ..MODE_STASH };
    let mut results = Vec::new();
    for file in &chosen {
        let current = read_worktree(&file.path, None)?;
        match apply_patch_file(file, &reverse, current.as_ref().map(|(e, b)| (e.1, b.as_slice())), "working directory") {
            // git applies `diff-tree -p HEAD <stash tree>` in reverse, whose header names HEAD's
            // mode, so a file it touches goes back to HEAD's mode.
            Ok(result) => results.push((file.path.clone(), result.map(|(c, m)| (c, file.old.map(|e| e.1).unwrap_or(m))))),
            Err(message) => {
                eprintln!("{}", message);
                eprintln!("Cannot remove worktree changes");
                crate::ui::exit(1);
            }
        }
    }
    for (path, result) in results {
        write_worktree(&path, result)?;
    }
    Ok(())
}

/// Splits `[<tree-ish>] [--] [<pathspec>...]` the way git does: a first argument before `--`
/// is a revision when it names one, otherwise every argument is a path.
fn split_rev_and_paths(args: &[String], skip: &[&str]) -> anyhow::Result<(Option<String>, Vec<String>)> {
    let args: Vec<&String> = args.iter().filter(|a| !skip.contains(&a.as_str())).collect();
    let dashdash = args.iter().position(|a| a.as_str() == "--");
    let (before, after): (Vec<String>, Vec<String>) = match dashdash {
        Some(i) => (args[..i].iter().map(|s| s.to_string()).collect(), args[i + 1..].iter().map(|s| s.to_string()).collect()),
        None => (args.iter().map(|s| s.to_string()).collect(), Vec::new()),
    };
    if let Some(opt) = before.iter().find(|a| a.starts_with('-')) {
        anyhow::bail!("error: unknown option '{}'", opt.trim_start_matches('-'));
    }
    let mut before = before.into_iter();
    let first = before.next();
    let mut paths: Vec<String>;
    let rev = match first {
        Some(f) if dashdash.is_some() || resolve_tree_from_source(&f).is_ok() => {
            if resolve_tree_from_source(&f).is_err() {
                anyhow::bail!("fatal: invalid reference: {}", f);
            }
            paths = before.collect();
            Some(f)
        }
        Some(f) => {
            paths = vec![f];
            paths.extend(before);
            None
        }
        None => {
            paths = Vec::new();
            None
        }
    };
    paths.extend(after);
    Ok((rev, paths))
}

fn hex_to_bytes(hash: &str) -> [u8; 20] {
    let mut out = [0u8; 20];
    out.copy_from_slice(&hex::decode(hash).unwrap_or_else(|_| vec![0; 20]));
    out
}

fn head_tree() -> anyhow::Result<TreeMap> {
    let mut map = TreeMap::new();
    if let Some(commit) = refs::resolve_head_commit()? {
        flatten_tree(&tree_hash_of_commit(&commit)?, "", &mut map)?;
    }
    Ok(map)
}

/// The index as path -> (blob, mode), without submodules and skip-worktree entries.
fn index_map() -> TreeMap {
    read_index()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.mode != 0o160000 && !e.skip_worktree)
        .map(|e| (e.path, (e.hash, e.mode)))
        .collect()
}

/// A working-tree file as (blob id, mode) and content; `None` when it is not a file.
fn read_worktree(path: &str, known_mode: Option<u32>) -> anyhow::Result<Option<(([u8; 20], u32), Vec<u8>)>> {
    let p = Path::new(path);
    let Ok(meta) = fs::symlink_metadata(p) else { return Ok(None) };
    if meta.is_dir() {
        return Ok(None);
    }
    let bytes = fs::read(p)?;
    Ok(Some(((hash_content("blob", &bytes), worktree_file_mode(&meta, known_mode)), bytes)))
}

fn write_worktree(path: &str, result: Option<(Vec<u8>, u32)>) -> anyhow::Result<()> {
    let p = Path::new(path);
    match result {
        None => {
            if p.exists() {
                fs::remove_file(p)?;
            }
        }
        Some((content, mode)) => {
            if let Some(parent) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                fs::create_dir_all(parent)?;
            }
            fs::write(p, &content)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perm = if mode == 0o100755 { 0o755 } else { 0o644 };
                fs::set_permissions(p, fs::Permissions::from_mode(perm))?;
            }
            #[cfg(not(unix))]
            let _ = mode;
        }
    }
    Ok(())
}

fn write_index_entry(path: &str, result: Option<(Vec<u8>, u32)>) -> anyhow::Result<()> {
    let mut entries = read_index().unwrap_or_default();
    entries.retain(|e| e.path != path);
    if let Some((content, mode)) = result {
        let hash = write_object("blob", &content)?;
        entries.push(bare_index_entry(path, hex_to_bytes(&hash), mode));
        entries.sort_by(|a, b| a.path.cmp(&b.path));
    }
    write_index(&mut entries)
}

/// Both sides of every changed path (limited to the pathspecs), as patch-mode files in path
/// order. A working-tree side covers the paths of the index and of the other side.
fn collect_files(pre: &Side, post: &Side, paths: &[String], only_tree_paths: bool) -> anyhow::Result<Vec<InteractiveFile>> {
    let specs = normalize_pathspecs(paths);
    let index = index_map();
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for side in [pre, post] {
        match side {
            Side::Tree(t) => candidates.extend(t.keys().cloned()),
            Side::Index | Side::Worktree if !only_tree_paths => candidates.extend(index.keys().cloned()),
            _ => {}
        }
    }
    let tree_side = [pre, post].into_iter().find_map(|s| match s {
        Side::Tree(t) => Some(t),
        _ => None,
    });
    let mut files = Vec::new();
    for path in candidates {
        if !matches_ls_files_pathspec(&path, &specs) {
            continue;
        }
        let side = |s: &Side| -> anyhow::Result<Option<(([u8; 20], u32), Vec<u8>)>> {
            let entry = match s {
                Side::Tree(t) => t.get(&path).copied(),
                Side::Index => index.get(&path).copied(),
                // Only tracked paths have a working-tree side (git ignores untracked files);
                // `stash -p` tracks HEAD's paths through its scratch index.
                Side::Worktree => {
                    let tracked = if only_tree_paths { tree_side.and_then(|t| t.get(&path)).map(|e| e.1) } else { index.get(&path).map(|e| e.1) };
                    return match tracked {
                        Some(m) => read_worktree(&path, Some(m)),
                        None => Ok(None),
                    };
                }
            };
            match entry {
                Some((h, m)) if m == 0o160000 => Ok(Some(((h, m), Vec::new()))),
                Some((h, m)) => Ok(Some(((h, m), read_object(&hex::encode(h))?.1))),
                None => Ok(None),
            }
        };
        let (a, b) = (side(pre)?, side(post)?);
        if a.as_ref().map(|x| x.0) == b.as_ref().map(|x| x.0) {
            continue;
        }
        if a.as_ref().is_some_and(|x| x.0 .1 == 0o160000) || b.as_ref().is_some_and(|x| x.0 .1 == 0o160000) {
            continue;
        }
        let (old, old_bytes) = a.map(|(e, b)| (Some(e), b)).unwrap_or((None, Vec::new()));
        let (new, new_bytes) = b.map(|(e, b)| (Some(e), b)).unwrap_or((None, Vec::new()));
        // Modes whose diff git computes with -R (from the index or working tree to a tree-ish).
        let swapped = matches!(post, Side::Tree(_)) && !matches!(pre, Side::Tree(_));
        files.push(parse_patch_file(&path, old, old_bytes, new, new_bytes, swapped)?);
    }
    Ok(files)
}

/// Runs one patch mode from `pre` to `post`, applying each file's chosen hunks to the mode's
/// target as soon as the file is done.
fn run_mode(mode: &PatchModeSpec, pre: Side, post: Side, paths: &[String]) -> anyhow::Result<()> {
    let mut files = collect_files(&pre, &post, paths, false)?;
    run_patch_mode(mode, &mut files, &mut |file, ask| {
        let index_current = || -> anyhow::Result<Option<(u32, Vec<u8>)>> {
            match index_map().get(&file.path) {
                Some(&(h, m)) => Ok(Some((m, read_object(&hex::encode(h))?.1))),
                None => Ok(None),
            }
        };
        let worktree_current = || -> anyhow::Result<Option<(u32, Vec<u8>)>> {
            Ok(read_worktree(&file.path, index_map().get(&file.path).map(|e| e.1))?.map(|(e, b)| (e.1, b)))
        };
        let apply_to = |current: &Option<(u32, Vec<u8>)>, place: &str| apply_patch_file(file, mode, current.as_ref().map(|(m, b)| (*m, b.as_slice())), place);
        match mode.target {
            PatchTarget::Index => {
                let now = index_current()?;
                warn_mode_mismatch(file, mode, &now);
                match apply_to(&now, "index") {
                    Ok(result) => write_index_entry(&file.path, result)?,
                    Err(message) => apply_failed(&message),
                }
            }
            PatchTarget::Worktree => {
                let now = worktree_current()?;
                warn_mode_mismatch(file, mode, &now);
                match apply_to(&now, "working directory") {
                    Ok(result) => write_worktree(&file.path, result)?,
                    Err(message) => apply_failed(&message),
                }
            }
            PatchTarget::Both => {
                // git's apply_for_checkout: check both, then apply to both, or ask, or show.
                let (index_now, worktree_now) = (index_current()?, worktree_current()?);
                let (index_result, worktree_result) = (apply_to(&index_now, "index"), apply_to(&worktree_now, "working directory"));
                for (now, result) in [(&index_now, &index_result), (&worktree_now, &worktree_result)] {
                    warn_mode_mismatch(file, mode, now);
                    if let Err(message) = result {
                        eprintln!("{}", message);
                    }
                }
                match (index_result, worktree_result) {
                    (Ok(i), Ok(w)) => {
                        // Two more `apply` runs (index, then working tree), with their warnings.
                        warn_mode_mismatch(file, mode, &index_now);
                        write_index_entry(&file.path, i)?;
                        warn_mode_mismatch(file, mode, &worktree_now);
                        write_worktree(&file.path, w)?;
                    }
                    (Err(_), w) => {
                        eprintln!("The selected hunks do not apply to the index!");
                        match (ask("Apply them to the worktree anyway? "), w) {
                            (Some(true), Ok(w)) => {
                                warn_mode_mismatch(file, mode, &worktree_now);
                                write_worktree(&file.path, w)?
                            }
                            (Some(true), Err(message)) => apply_failed(&message),
                            _ => eprintln!("Nothing was applied.\n"),
                        }
                    }
                    (Ok(_), Err(_)) => print_selected_hunks(file),
                }
            }
            PatchTarget::Stash => unreachable!("stash -p has its own driver"),
        }
        Ok(())
    })?;
    Ok(())
}

/// `apply`'s warning when the target's mode is not the one the patch starts from (the patch
/// only names modes when its mode-change hunk was chosen).
fn warn_mode_mismatch(file: &InteractiveFile, mode: &PatchModeSpec, current: &Option<(u32, Vec<u8>)>) {
    if !(file.mode_change && file.hunks.first().is_some_and(|h| h.use_ == crate::objects::HunkUse::Use)) {
        return;
    }
    let expected = if mode.is_reverse { file.new } else { file.old }.map(|e| e.1);
    if let (Some((have, _)), Some(want)) = (current, expected) {
        if *have != want {
            eprintln!("warning: {} has type {:o}, expected {:o}", file.path, have, want);
        }
    }
}

/// git's last resort when the hunks apply to the index but not the working tree: show them.
fn print_selected_hunks(file: &InteractiveFile) {
    let mut text = Vec::new();
    for line in &file.header {
        text.extend_from_slice(line);
    }
    for hunk in file.hunks.iter().filter(|h| h.use_ == crate::objects::HunkUse::Use) {
        text.extend_from_slice(&crate::helpers::add_patch::render_hunk(hunk));
    }
    use std::io::Write;
    let _ = std::io::stdout().write_all(&text);
}

/// What git prints when its final `apply` fails: apply's own errors, then its own line.
fn apply_failed(message: &str) {
    eprintln!("{}", message);
    eprintln!("error: 'git apply' failed");
}
